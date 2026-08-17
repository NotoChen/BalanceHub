//! Shell execution boundary for temporary CLI launches.
//!
//! Terminal adapters decide where a script is opened. This module decides how the user's shell
//! environment is captured and how a portable launch script is rendered for Unix, PowerShell or
//! cmd without assigning to host-reserved variables.

pub(super) mod environment;
pub(super) mod script;
