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
                .map_err(|error| format!("无法启动 Windows 交互终端：{error}"))
        }
        #[cfg(not(windows))]
        {
            Err("当前系统不支持 Windows 终端".into())
        }
    }

    #[cfg(windows)]
    fn spawn_windows(&self) -> std::io::Result<()> {
        use std::{env, ffi::OsString, mem::size_of, os::windows::ffi::OsStrExt, ptr};
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
            CloseHandle(process.hProcess);
        }
        Ok(())
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
        use std::{
            fs, thread,
            time::{Duration, Instant},
        };
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("console-io.json");
        let script = "[System.IO.File]::WriteAllText($env:BALANCEHUB_TEMPORARY_CLI_SCRIPT, (@{ inputRedirected = [Console]::IsInputRedirected; outputRedirected = [Console]::IsOutputRedirected } | ConvertTo-Json -Compress))";
        let arguments = format!(
            "-NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
            crate::platform::windows_shell::encoded_powershell(script)
        );
        ConsoleCommand::new("powershell.exe", &arguments, &output)
            .unwrap()
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let value = loop {
            if let Some(value) = fs::read(&output)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            {
                break value;
            }
            assert!(
                Instant::now() < deadline,
                "new console did not publish its handle state"
            );
            thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(value["inputRedirected"], false);
        assert_eq!(value["outputRedirected"], false);
    }
}
