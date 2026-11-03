//! Session integration and the global asset catalog share native write domains.

use crate::services::agent_cli::environment::mutation::{locking::shared_locks, GuardedFile};
use std::{
    path::Path,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

pub(super) fn with_locked_sources<T>(
    config: &Path,
    manifest: &Path,
    config_root: &Path,
    action: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let capture = |path: &Path| {
        GuardedFile::capture_path(path.parent().ok_or("Hook 配置目录无效")?, path, 1024 * 1024)
            .map_err(|_| "Hook 配置路径不可安全访问，未执行变更".to_owned())
    };
    let config_file = capture(config)?;
    let manifest_file = capture(manifest)?;
    let mut domains = config_file.lock_domains();
    domains.extend(manifest_file.lock_domains());
    domains.push(format!("config:{}", config_root.display()));
    let locks = shared_locks();
    let canceled = AtomicBool::new(false);
    let _guard = locks
        .acquire(
            &domains,
            &canceled,
            Instant::now() + Duration::from_secs(15),
        )
        .map_err(|_| "等待 Hook 配置写入超时，请刷新后重试".to_owned())?;
    config_file
        .revalidate()
        .and_then(|()| manifest_file.revalidate())
        .map_err(|_| "Hook 配置或所有权已变化，请重新生成计划".to_owned())?;
    action()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AgentHookMutation;
    use crate::services::agent_runtime::managed_hook::CodexHookService;
    use std::{fs, sync::mpsc, thread};

    #[test]
    fn session_writer_waits_for_catalog_domain_and_rejects_changed_plan() {
        let directory = tempfile::tempdir().unwrap();
        let root = &directory.path().canonicalize().unwrap();
        fs::create_dir(root.join(".codex")).unwrap();
        fs::create_dir(root.join("app")).unwrap();
        let config = root.join(".codex/hooks.json");
        let helper = root.join("app/balancehub");
        fs::write(&config, b"{}").unwrap();
        fs::write(&helper, b"helper").unwrap();
        let service = CodexHookService::new(
            config.clone(),
            root.join("app/ownership.json"),
            helper,
            root.join("app"),
        );
        let plan = service.plan(AgentHookMutation::Install);
        let file = GuardedFile::capture_path(root, &config, 1024).unwrap();
        let locks = shared_locks();
        let canceled = AtomicBool::new(false);
        let guard = locks
            .acquire(
                &[file.domain()],
                &canceled,
                Instant::now() + Duration::from_secs(1),
            )
            .unwrap();
        let (sender, receiver) = mpsc::channel();
        let writer = thread::spawn(move || sender.send(service.apply(plan)).unwrap());
        assert!(matches!(
            receiver.recv_timeout(Duration::from_millis(30)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        // Simulate an already-confirmed catalog write in the held domain.
        let current = br#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"user-rule"}]}]},"unrelated":true}"#;
        fs::write(&config, current).unwrap();
        drop(guard);
        assert!(receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_err());
        writer.join().unwrap();
        assert_eq!(fs::read(&config).unwrap(), current);
        assert!(!root.join("app/ownership.json").exists());
    }
}
