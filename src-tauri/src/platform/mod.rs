pub(crate) mod cc_switch;
pub(crate) mod paths;
pub(crate) mod process;
#[cfg(any(windows, test))]
pub(crate) mod windows_console;
#[cfg(windows)]
pub(crate) mod windows_registry;
#[cfg(any(windows, test))]
pub(crate) mod windows_shell;
