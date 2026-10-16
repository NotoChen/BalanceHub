use super::paths::windows_shell_path;
use std::path::{Path, PathBuf};

pub(crate) const SCRIPT_ENV: &str = "BALANCEHUB_TEMPORARY_CLI_SCRIPT";
pub(crate) const CMD_SCRIPT_ARGUMENTS: &str =
    r#"/D /E:ON /V:OFF /S /K ""%BALANCEHUB_TEMPORARY_CLI_SCRIPT%"""#;

pub(crate) struct ConsoleCommand {
    pub command_line: String,
    pub script: PathBuf,
}

impl ConsoleCommand {
    pub(crate) fn new(program: &str, arguments: &str, script: &Path) -> Result<Self, String> {
        if program.is_empty()
            || !program
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            || arguments.contains('\0')
        {
            return Err("Windows 终端启动参数无效".into());
        }
        Ok(Self {
            command_line: format!("\"{program}\" {arguments}"),
            script: windows_shell_path(script)?.into_owned(),
        })
    }

    pub(crate) fn spawn(&self) -> Result<(), String> {
        #[cfg(windows)]
        {
            self.spawn_windows()
                .map(drop)
                .map_err(|error| format!("无法启动 Windows 交互终端：{error}"))
        }
        #[cfg(not(windows))]
        {
            Err("当前系统不支持 Windows 终端".into())
        }
    }

    #[cfg(windows)]
    fn spawn_windows(&self) -> std::io::Result<std::os::windows::io::OwnedHandle> {
        use std::{
            env,
            ffi::OsString,
            mem::size_of,
            os::windows::{
                ffi::OsStrExt,
                io::{FromRawHandle, OwnedHandle},
            },
            ptr,
        };
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Threading::{
                CreateProcessW, CREATE_NEW_CONSOLE, CREATE_UNICODE_ENVIRONMENT,
                PROCESS_INFORMATION, STARTUPINFOW,
            },
        };

        let mut variables = env::vars_os()
            .filter(|(name, _)| !name.eq_ignore_ascii_case(SCRIPT_ENV))
            .collect::<Vec<_>>();
        variables.push((
            OsString::from(SCRIPT_ENV),
            self.script.as_os_str().to_owned(),
        ));
        variables.sort_by_cached_key(|(name, _)| name.to_string_lossy().to_uppercase());
        let mut environment = Vec::<u16>::new();
        for (name, value) in variables {
            environment.extend(name.encode_wide());
            environment.push(u16::from(b'='));
            environment.extend(value.encode_wide());
            environment.push(0);
        }
        environment.push(0);
        let mut command_line = self
            .command_line
            .encode_utf16()
            .chain([0])
            .collect::<Vec<_>>();
        // CMD cannot inherit a UNC cwd, even when the script itself can be read
        // from a share. The wrapper enters the requested workspace with pushd.
        let directory = env::var_os("SystemRoot")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "无法定位 Windows 系统目录")
            })?;
        let directory = super::paths::compatible_path(&directory)
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        let startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut process = PROCESS_INFORMATION::default();
        // No STARTF_USESTDHANDLES and no inherited handles: Windows supplies
        // interactive input/output for the new console, even under tauri dev.
        let started = unsafe {
            CreateProcessW(
                ptr::null(),
                command_line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                CREATE_NEW_CONSOLE | CREATE_UNICODE_ENVIRONMENT,
                environment.as_ptr().cast(),
                directory.as_ptr(),
                &startup,
                &mut process,
            )
        };
        if started == 0 {
            return Err(std::io::Error::last_os_error());
        }
        unsafe {
            CloseHandle(process.hThread);
        }
        Ok(unsafe { OwnedHandle::from_raw_handle(process.hProcess) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_paths_are_data_instead_of_nested_cmd_source() {
        let script = Path::new(r"C:\用户\100%PATH% ! (demo) & test\codex.cmd");
        let command = ConsoleCommand::new("cmd.exe", CMD_SCRIPT_ARGUMENTS, script).unwrap();
        assert_eq!(command.script, script);
        assert!(!command.command_line.contains("100%PATH%"));
        assert!(command.command_line.contains(SCRIPT_ENV));
        assert!(!command.command_line.contains("start"));
    }

    #[cfg(windows)]
    #[test]
    fn new_console_receives_interactive_standard_handles() {
        use std::{env, fs, os::windows::io::AsRawHandle};
        use windows_sys::Win32::{
            Foundation::{WAIT_FAILED, WAIT_OBJECT_0},
            System::Threading::{GetExitCodeProcess, TerminateProcess, WaitForSingleObject},
        };

        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("console-io.json");
        let executable = env::current_exe().unwrap();
        let program = executable.file_name().unwrap().to_str().unwrap();
        let module = module_path!().split_once("::").unwrap().1;
        // Reuse the native test executable so this console probe does not
        // depend on PowerShell startup during the parallel Windows test suite.
        let arguments = format!("--exact {module}::console_handle_probe --ignored --nocapture");
        let process = ConsoleCommand::new(program, &arguments, &output)
            .unwrap()
            .spawn_windows()
            .unwrap();
        let wait = unsafe { WaitForSingleObject(process.as_raw_handle(), 10_000) };
        if wait != WAIT_OBJECT_0 {
            let error = (wait == WAIT_FAILED).then(std::io::Error::last_os_error);
            unsafe {
                TerminateProcess(process.as_raw_handle(), 1);
                WaitForSingleObject(process.as_raw_handle(), 1_000);
            }
            panic!("console probe did not exit: wait={wait}, error={error:?}");
        }
        let mut exit_code = 0;
        assert_ne!(
            unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut exit_code) },
            0,
            "could not read console probe exit code: {}",
            std::io::Error::last_os_error()
        );
        assert_eq!(exit_code, 0, "console probe failed");
        let bytes = fs::read(&output).expect("console probe did not publish its handle state");
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).expect("console probe returned invalid handle state");
        assert_eq!(value["inputRedirected"], false);
        assert_eq!(value["outputRedirected"], false);
        assert_eq!(value["errorRedirected"], false);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "子进程探针，由 new_console_receives_interactive_standard_handles 显式执行"]
    fn console_handle_probe() {
        use std::{env, fs, io::IsTerminal};

        let output = env::var_os(SCRIPT_ENV).expect("console probe output path is missing");
        let value = serde_json::json!({
            "inputRedirected": !std::io::stdin().is_terminal(),
            "outputRedirected": !std::io::stdout().is_terminal(),
            "errorRedirected": !std::io::stderr().is_terminal(),
        });
        fs::write(output, serde_json::to_vec(&value).unwrap()).unwrap();
    }
}
