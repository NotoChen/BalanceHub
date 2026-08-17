use crate::services::temporary_cli::shell_runtime::script::{
    script_command, script_command_without_exec,
};
use std::path::Path;

pub(crate) fn build_macos_terminal_applescript(script: &Path) -> String {
    let launcher = apple_script_exec_launcher_command(script);
    format!(
        r#"set launcher_script to {launcher}
set was_running to application "Terminal" is running
tell application "Terminal"
    if was_running then
        activate
        do script launcher_script
    else
        launch
        do script launcher_script
        activate
    end if
end tell"#,
    )
}

pub(crate) fn build_macos_iterm2_applescript(script: &Path) -> String {
    let launcher = apple_script_exec_launcher_command(script);
    format!(
        r#"set launcher_script to {launcher}
set was_running to application "iTerm" is running
tell application "iTerm"
    if was_running then
        activate
        if (count of windows) = 0 then
            create window with default profile
        else
            tell current window
                create tab with default profile
            end tell
        end if
    else
        activate
        set waited to 0
        repeat while (count of windows) = 0
            delay 0.1
            set waited to waited + 1
            if waited >= 30 then exit repeat
        end repeat
        if (count of windows) = 0 then
            create window with default profile
        end if
    end if
    tell current session of current window
        write text launcher_script
    end tell
end tell"#,
    )
}

pub(crate) fn build_macos_ghostty_applescript(script: &Path) -> String {
    let launcher = apple_script_launcher_command(script);
    format!(
        r#"set launcher_command to {launcher}
tell application "Ghostty"
    set target_window to new window with configuration {{command:launcher_command}}
    set target_tab to selected tab of target_window
    set target_terminal to focused terminal of target_tab
    activate
    return id of target_terminal
end tell"#,
    )
}

pub(crate) fn build_macos_ghostty_activation_applescript(terminal_id: &str) -> String {
    let target_id = apple_script_quote(terminal_id);
    format!(
        r#"set target_id to {target_id}
tell application "Ghostty"
    set matching_terminals to every terminal whose id is target_id
    if (count of matching_terminals) is 0 then error "terminal not found"
    focus item 1 of matching_terminals
    activate
end tell"#,
    )
}

fn apple_script_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn apple_script_launcher_command(script: &Path) -> String {
    apple_script_quote(&script_command_without_exec(script))
}

fn apple_script_exec_launcher_command(script: &Path) -> String {
    apple_script_quote(&script_command(script))
}
