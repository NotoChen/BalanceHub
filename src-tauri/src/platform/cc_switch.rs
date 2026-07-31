use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

const OPEN_ERROR: &str = "系统没有成功打开 CC Switch 深链";

#[cfg(target_os = "macos")]
pub(crate) fn open(app: &AppHandle, url: &str) -> Result<(), String> {
    use crate::{limits, platform::process::run_command_with_output_timeout};
    use std::{process::Command, time::Duration};

    const CC_SWITCH_BUNDLE_ID: &str = "com.ccswitch.desktop";
    let mut command = Command::new("open");
    command.args(["-b", CC_SWITCH_BUNDLE_ID, url]);
    let preferred_error = match run_command_with_output_timeout(
        &mut command,
        Duration::from_secs(10),
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    ) {
        Ok(output) if output.status.is_some_and(|status| status.success()) => return Ok(()),
        Ok(output) if output.timed_out => "命令执行超时".to_string(),
        Ok(output) => command_error(
            output.stderr.as_bytes(),
            output.status.and_then(|status| status.code()),
        ),
        Err(err) => err.to_string(),
    };

    open_with_default_handler(app, url).map_err(|fallback_error| {
        format!(
            "{OPEN_ERROR}: bundle id {CC_SWITCH_BUNDLE_ID}: {preferred_error}; 默认处理器: {fallback_error}"
        )
    })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn open(app: &AppHandle, url: &str) -> Result<(), String> {
    open_with_default_handler(app, url).map_err(|err| format!("{OPEN_ERROR}: {err}"))
}

fn open_with_default_handler(app: &AppHandle, url: &str) -> Result<(), String> {
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|err| err.to_string())
}

#[cfg(target_os = "macos")]
fn command_error(stderr: &[u8], status: Option<i32>) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    if stderr.is_empty() {
        status
            .map(|code| format!("退出码 {code}"))
            .unwrap_or_else(|| "命令执行失败".to_string())
    } else {
        stderr
    }
}
