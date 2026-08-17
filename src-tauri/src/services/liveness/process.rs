use crate::{
    limits,
    platform::process::{
        configure_process_group as configure_shared_process_group,
        wait_with_output_timeout as wait_with_shared_output_timeout, CommandOutput,
    },
};
use std::{process::Command, time::Duration};

pub(super) fn configure_process_group(command: &mut Command) {
    configure_shared_process_group(command);
}

pub(super) fn wait_with_output_timeout(
    child: std::process::Child,
    timeout: Duration,
) -> CommandOutput {
    wait_with_shared_output_timeout(child, timeout, limits::MAX_COMMAND_OUTPUT_BYTES)
}
