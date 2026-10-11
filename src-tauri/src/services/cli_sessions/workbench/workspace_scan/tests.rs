use super::*;
use crate::services::cli_sessions::workbench::{
    scope::{history_adapter, workspace_scope},
    with_read_budget, with_source_root,
};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::symlink,
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};

fn write_record(kind: AgentCliKind, root: &Path, workdir: &Path, id: &str) {
    match kind {
        AgentCliKind::Codex => {
            let database = rusqlite::Connection::open(root.join("state_5.sqlite")).unwrap();
            database.execute_batch("CREATE TABLE IF NOT EXISTS threads (id TEXT PRIMARY KEY, cwd TEXT, title TEXT, rollout_path TEXT, source TEXT)").unwrap();
            let path = root.join(format!("rollout-{id}.jsonl"));
            fs::write(&path, format!("{}\n", json!({"type":"session_meta", "payload":{"id":id, "cwd":workdir, "source":"cli"}}))).unwrap();
            database
                .execute(
                    "INSERT OR IGNORE INTO threads VALUES (?1, ?2, ?1, ?3, 'cli')",
                    rusqlite::params![id, workdir.to_string_lossy(), path.to_string_lossy()],
                )
                .unwrap();
        }
        AgentCliKind::ClaudeCode => {
            let encoded = workdir
                .to_string_lossy()
                .chars()
                .map(|value| {
                    if value.is_ascii_alphanumeric() {
                        value
                    } else {
                        '-'
                    }
                })
                .collect::<String>();
            let project = root.join("projects").join(encoded);
            fs::create_dir_all(&project).unwrap();
            fs::write(project.join(format!("{id}.jsonl")), format!("{}\n", json!({"type":"user", "sessionId":id, "cwd":workdir, "isSidechain":false, "message":{"content":id}}))).unwrap();
        }
        AgentCliKind::Grok => {
            let encoded = workdir
                .to_string_lossy()
                .bytes()
                .map(|value| {
                    if value.is_ascii_alphanumeric() || b"-_.~".contains(&value) {
                        char::from(value).to_string()
                    } else {
                        format!("%{value:02X}")
                    }
                })
                .collect::<String>();
            let session = root.join("sessions").join(encoded).join(id);
            fs::create_dir_all(&session).unwrap();
            fs::write(
                session.join("summary.json"),
                json!({"info":{"id":id, "cwd":workdir}, "generated_title":id, "num_messages":1})
                    .to_string(),
            )
            .unwrap();
        }
        AgentCliKind::Gemini => unreachable!(),
    }
}

#[test]
fn explicit_native_scans_discover_registered_aliases_on_first_query_and_reject_retargeting() {
    for kind in [
        AgentCliKind::Codex,
        AgentCliKind::ClaudeCode,
        AgentCliKind::Grok,
    ] {
        let root = std::env::temp_dir().join(format!(
            "balancehub-session-alias-scan-{}-{}",
            kind.key(),
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let physical = root.join("physical");
        let alias = root.join("alias");
        let child = physical.join("unregistered-child");
        let native = root.join("native");
        let outside = root.join("outside");
        let home = root.join("home");
        for directory in [&child, &native, &outside, &home] {
            fs::create_dir_all(directory).unwrap();
        }
        symlink(&physical, &alias).unwrap();
        assert!(
            !state::registry()
                .path_identities
                .contains_key(alias.to_string_lossy().as_ref()),
            "{kind:?}: the first query must not depend on a previously resolved alias"
        );
        let workspaces = workspace_scope(
            &home,
            &[alias.clone(), outside.clone()],
            Some(physical.to_str().unwrap()),
        )
        .unwrap();
        assert_eq!(workspaces.len(), 1, "{kind:?}: explicit scope stays narrow");
        assert_eq!(workspaces[0].path, physical.to_string_lossy());
        for (directory, id) in [
            (&alias, "alias-session"),
            (&physical, "physical-session"),
            (&alias, "shared"),
            (&physical, "shared"),
            (&child, "unregistered-session"),
            (&outside, "other-registered-session"),
        ] {
            write_record(kind, &native, directory, id);
        }
        let read = |mode| {
            let budget = SessionReadBudget::new(
                Instant::now() + Duration::from_secs(10),
                Arc::new(AtomicBool::new(false)),
                16 * 1024 * 1024,
            );
            with_read_budget(&budget, || {
                with_source_root(&native, || {
                    scan_workspaces(
                        history_adapter(kind).unwrap(),
                        kind,
                        &native,
                        &workspaces,
                        &budget,
                        mode,
                    )
                })
            })
            .unwrap()
        };
        for mode in [
            SessionHistoryReadMode::Summaries,
            SessionHistoryReadMode::Identities,
        ] {
            let result = read(mode);
            assert_eq!(result.len(), 1, "{kind:?}: aliases form one workspace");
            assert!(result[0].complete, "{kind:?}: {:?}", result[0].diagnostics);
            let ids = result[0]
                .records
                .iter()
                .map(|record| record.summary.id.as_str())
                .collect::<BTreeSet<_>>();
            assert_eq!(
                ids,
                BTreeSet::from(["alias-session", "physical-session", "shared"]),
                "{kind:?}"
            );
            assert_eq!(
                result[0].records.len(),
                3,
                "{kind:?}: duplicates must not reach counts or indexing"
            );
        }
        fs::remove_file(&alias).unwrap();
        symlink(&outside, &alias).unwrap();
        let result = read(SessionHistoryReadMode::Summaries);
        assert!(
            result[0]
                .records
                .iter()
                .all(|record| record.summary.id != "alias-session"),
            "{kind:?}: changed alias must not retain the old directory grant"
        );
        assert!(result[0]
            .records
            .iter()
            .any(|record| record.summary.id == "physical-session"));
        fs::remove_dir_all(root).unwrap();
    }
}
