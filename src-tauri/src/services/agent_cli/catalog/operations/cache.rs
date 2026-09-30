//! Account every retained private buffer, including original target files and
//! replacements. Similar sources do not make separately owned target copies free.
use super::Work;
use crate::services::agent_cli::environment::mutation::{MutationExecution, PreparedMutation};
use serde::Serialize;

pub(in crate::services::agent_cli::catalog) fn encoded_bytes<T: Serialize + ?Sized>(
    value: &T,
) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

pub(super) fn work_bytes(work: &[Work]) -> usize {
    work.iter().fold(0_usize, |total, work| {
        total.saturating_add(match work {
            Work::NativeFiles(members) | Work::NativeCli(members) => {
                members.iter().fold(0_usize, |total, member| {
                    total
                        .saturating_add(native_bytes(&member.prepared))
                        .saturating_add(encoded_bytes(&member.request))
                })
            }
            Work::Distribution(targets) => targets.iter().fold(0_usize, |total, (_, target)| {
                total.saturating_add(target.retained_private_bytes())
            }),
            Work::Removal(targets) => targets.iter().fold(0_usize, |total, (_, target)| {
                total.saturating_add(target.retained_private_bytes())
            }),
            Work::Hooks(group) => group.retained_private_bytes(),
        })
    })
}

fn native_bytes(prepared: &PreparedMutation) -> usize {
    let files = prepared.files.iter().fold(0_usize, |total, file| {
        total.saturating_add(file.bytes().map_or(0, <[u8]>::len))
    });
    let execution = match &prepared.execution {
        MutationExecution::AtomicFile { replacement, .. } => replacement.len(),
        MutationExecution::ExactCli(command) => command
            .argv
            .iter()
            .fold(0_usize, |total, argument| {
                total.saturating_add(argument.len())
            })
            .saturating_add(
                command
                    .environment
                    .iter()
                    .fold(0_usize, |total, (name, value)| {
                        total.saturating_add(name.len()).saturating_add(value.len())
                    }),
            )
            .saturating_add(command.cwd.as_os_str().len()),
    };
    files
        .saturating_add(execution)
        .saturating_add(encoded_bytes(&prepared.changes))
}
