use crate::{
    limits,
    platform::process::{
        run_command_with_output_timeout as run_shared_command_with_output_timeout, CommandOutput,
    },
};
use std::{process::Command, time::Duration};

pub(super) fn run_command_with_output_timeout(
    command: &mut Command,
    timeout: Duration,
) -> std::io::Result<CommandOutput> {
    run_shared_command_with_output_timeout(command, timeout, limits::MAX_COMMAND_OUTPUT_BYTES)
}
