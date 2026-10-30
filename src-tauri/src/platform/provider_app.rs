use crate::models::ProviderAppImportTarget;

#[cfg(target_os = "macos")]
pub(crate) fn open(target: ProviderAppImportTarget, url: &str) -> Result<(), String> {
    let bundle_id = match target {
        ProviderAppImportTarget::Magpie => "com.yetone.magpie",
        ProviderAppImportTarget::CcSwitch(_) => "com.ccswitch.desktop",
    };
    if run_opener("/usr/bin/open", &["-b", bundle_id, url], target)?
        || run_opener("/usr/bin/open", &[url], target)?
    {
        Ok(())
    } else {
        Err(unavailable_message(target))
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn open(target: ProviderAppImportTarget, url: &str) -> Result<(), String> {
    use crate::{limits, platform::process::run_command_with_output_timeout};
    use std::{process::Command, time::Duration};

    if run_opener("gio", &["open", url], target)? {
        return Ok(());
    }
    let mime = match target {
        ProviderAppImportTarget::Magpie => "x-scheme-handler/magpie",
        ProviderAppImportTarget::CcSwitch(_) => "x-scheme-handler/ccswitch",
    };
    let mut query = Command::new("xdg-mime");
    query.args(["query", "default", mime]);
    let registered = run_command_with_output_timeout(
        &mut query,
        Duration::from_secs(5),
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    )
    .is_ok_and(|output| {
        !output.timed_out
            && output.status.is_some_and(|status| status.success())
            && !output.stdout.trim().is_empty()
    });
    if !registered {
        return Err(unavailable_message(target));
    }
    // Some xdg-open handlers stay attached for the app's lifetime. Detach
    // after checking registration so a timeout cannot close its import dialog.
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|_| unavailable_message(target))
}

#[cfg(not(windows))]
fn run_opener(
    program: &str,
    args: &[&str],
    target: ProviderAppImportTarget,
) -> Result<bool, String> {
    use crate::{limits, platform::process::run_command_with_output_timeout};
    use std::{process::Command, time::Duration};

    let mut command = Command::new(program);
    command.args(args);
    match run_command_with_output_timeout(
        &mut command,
        Duration::from_secs(10),
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    ) {
        Ok(output) if output.timed_out => Err(format!(
            "系统打开请求超时，请检查 {} 是否已打开",
            target.app_label()
        )),
        Ok(output) => Ok(output.status.is_some_and(|status| status.success())),
        // System opener errors can echo the full credential-bearing URL.
        // Never return stdout/stderr or raw process errors through IPC.
        Err(_) => Ok(false),
    }
}

#[cfg(not(windows))]
fn unavailable_message(target: ProviderAppImportTarget) -> String {
    format!(
        "系统未能打开 {}，请确认已安装并手动启动过应用，再重试",
        target.app_label()
    )
}

#[cfg(windows)]
pub(crate) fn open(target: ProviderAppImportTarget, url: &str) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::GetLastError,
        UI::Shell::{ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW},
    };

    // Native UTF-16 launching preserves percent-encoded names and keys. cmd
    // start would expand percent sequences in the URL as environment variables.
    let url = url.encode_utf16().chain([0]).collect::<Vec<_>>();
    let verb = "open".encode_utf16().chain([0]).collect::<Vec<_>>();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
        lpVerb: verb.as_ptr(),
        lpFile: url.as_ptr(),
        nShow: 1, // SW_SHOWNORMAL
        ..Default::default()
    };
    if unsafe { ShellExecuteExW(&mut info) } != 0 {
        Ok(())
    } else {
        let code = unsafe { GetLastError() };
        Err(format!(
            "Windows 无法打开 {}（系统错误 {code}），请确认已安装并手动启动过应用",
            target.app_label()
        ))
    }
}
