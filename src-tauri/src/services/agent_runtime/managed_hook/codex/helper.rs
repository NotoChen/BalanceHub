use super::ownership::now_millis;
use crate::services::agent_runtime::{
    decoders::decoder_for_agent,
    hook::{ingest_payload_with_context, HookIngestContext, HookSpoolRepository},
};
use std::{
    env,
    io::{self, Read},
    path::Path,
    sync::mpsc,
    time::Duration,
};

const HOOK_STDIN_TIMEOUT: Duration = Duration::from_secs(2);

/// Hidden helper entry point. It never initializes Tauri or writes output:
/// Agent Hooks may treat stdout as context, so every failure is intentionally
/// swallowed so a broken BalanceHub install cannot block the Agent.
pub(super) fn run() -> Option<i32> {
    let args = env::args().collect::<Vec<_>>();
    if !args.iter().any(|arg| arg == "--balancehub-hook-ingest") {
        return None;
    }
    let get_arg = |name: &str| {
        args.windows(2)
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].clone())
    };
    let Some(agent) = get_arg("--agent") else {
        return Some(0);
    };
    let Some(decoder) = decoder_for_agent(&agent) else {
        return Some(0);
    };
    let Some(spool_root) = get_arg("--spool-root") else {
        return Some(0);
    };
    let _ = (|| {
        let repository = HookSpoolRepository::new(Path::new(&spool_root)).map_err(|_| ())?;
        let context = HookIngestContext::from_process_env_with_validator(
            crate::services::cli_runtime::instance_exists,
        );
        let payload = read_stdin_bounded(repository.limits().max_event_bytes).ok_or(())?;
        let _ = ingest_payload_with_context(decoder, &payload, now_millis(), &repository, &context);
        Ok::<(), ()>(())
    })();
    Some(0)
}

/// The helper is a short-lived subprocess, so a timed-out reader can be
/// detached safely: returning from `run` exits the process and closes stdin.
/// This prevents a broken Agent from holding the Hook helper indefinitely.
fn read_stdin_bounded(limit: usize) -> Option<Vec<u8>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("balancehub-hook-stdin".to_string())
        .spawn(move || {
            let mut stdin = io::stdin().lock();
            let mut payload = Vec::with_capacity(limit.min(16 * 1024));
            let result = Read::by_ref(&mut stdin)
                .take((limit as u64).saturating_add(1))
                .read_to_end(&mut payload)
                .ok()
                .filter(|_| payload.len() <= limit)
                .map(|_| payload);
            let _ = sender.send(result);
        })
        .ok()?;
    receiver.recv_timeout(HOOK_STDIN_TIMEOUT).ok().flatten()
}
