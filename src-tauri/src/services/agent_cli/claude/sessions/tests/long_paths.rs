use super::super::{history::HISTORY, parse_transcript};
use crate::{
    models::{AgentCliKind, AgentSessionRole},
    services::{
        agent_cli::contracts::{
            SessionContentSearchRequest, SessionHistoryWorkspace, SessionReadBudget,
            SessionSearchTerm,
        },
        cli_sessions::workbench::with_read_budget,
    },
};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "balancehub-claude-long-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("projects")).unwrap();
        Self(root)
    }
    fn write(&self, directory: &str, file: &str, values: &[serde_json::Value]) -> PathBuf {
        let path = self.0.join("projects").join(directory).join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        path
    }
    fn scan(&self, workdirs: &[PathBuf]) -> Vec<SessionHistoryWorkspace> {
        let budget = SessionReadBudget::new(
            Instant::now() + Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
            4 * 1024 * 1024,
        );
        with_read_budget(&budget, || {
            (HISTORY.scan)(
                AgentCliKind::ClaudeCode,
                &self.0,
                workdirs,
                &budget,
                crate::services::agent_cli::contracts::SessionHistoryReadMode::Summaries,
            )
        })
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn prefixes() -> (&'static str, &'static str) {
    if cfg!(target_os = "windows") {
        ("C:\\fixture\\", "C--fixture-")
    } else {
        ("/fixture/", "-fixture-")
    }
}
fn workdir(length: usize, suffix: &str) -> PathBuf {
    let (native, _) = prefixes();
    PathBuf::from(format!(
        "{native}{}{suffix}",
        "a".repeat(length - native.len() - suffix.len())
    ))
}
fn directory(length: usize, hash: &str) -> String {
    let (_, native) = prefixes();
    if length <= 200 {
        format!("{native}{}", "a".repeat(length - native.len()))
    } else {
        format!("{native}{}-{hash}", "a".repeat(200 - native.len()))
    }
}

#[test]
fn native_project_length_boundaries_keep_main_child_bodies_and_parent_identity() {
    let fixture = Fixture::new("boundaries");
    for (length, hash) in [(199, ""), (200, ""), (201, "bun-one"), (240, "sdk-two")] {
        let cwd = workdir(length, "");
        let name = directory(length, hash);
        let id = format!("parent-{length}");
        fixture.write(&name, &format!("{id}.jsonl"), &[json!({"sessionId":id,"cwd":cwd,"type":"user","message":{"content":format!("parent body {length}")}})]);
        fixture.write(&name, &format!("{id}/subagents/agent-child-{length}.jsonl"), &[json!({"sessionId":id,"cwd":cwd,"isSidechain":true,"type":"assistant","message":{"content":format!("child body {length}")}})]);
    }
    fixture.write(&directory(201, ""), "empty-hash.jsonl", &[json!({"sessionId":"empty-hash","cwd":workdir(201, ""),"type":"user","message":{"content":"empty suffix is not a native candidate"}})]);
    for length in [199, 200, 201, 240] {
        let cwd = workdir(length, "");
        let groups = fixture.scan(std::slice::from_ref(&cwd));
        assert_eq!(groups.len(), 1);
        assert!(groups[0].complete, "{:?}", groups[0].diagnostics);
        assert_eq!(groups[0].records.len(), 2, "length {length}");
        for record in &groups[0].records {
            assert_eq!(record.summary.workdir, cwd.to_string_lossy());
            let body = if record.role == AgentSessionRole::Main {
                format!("parent body {length}")
            } else {
                assert_eq!(
                    record.parent_native_id.as_deref(),
                    Some(format!("parent-{length}").as_str())
                );
                assert!(!record.summary.can_resume);
                format!("child body {length}")
            };
            let detail = (HISTORY.detail)(record, super::TEST_LIMITS).unwrap();
            assert_eq!(detail.messages.len(), 1);
            assert_eq!(detail.messages[0].content, body);
            let found = (HISTORY.search)(
                record,
                &SessionContentSearchRequest {
                    terms: vec![SessionSearchTerm {
                        index: 0,
                        value: body,
                    }],
                },
                &|| true,
            )
            .unwrap();
            assert_eq!(found.matched_term_indexes, vec![0]);
        }
    }
}

#[test]
fn prefix_candidates_require_first_valid_native_origin_not_later_directory_changes() {
    let fixture = Fixture::new("origin");
    let a = workdir(240, "A");
    let b = workdir(240, "B");
    let a_dir = directory(240, "bun-a");
    let b_dir = directory(240, "sdk-b");
    let changed = fixture.write(&a_dir, "changed.jsonl", &[
        json!({"sessionId":"changed","cwd":"relative","type":"metadata"}),
        json!({"sessionId":"changed","workdir":a,"type":"user","message":{"content":"origin A"}}),
        json!({"sessionId":"changed","cwd":b,"type":"assistant","message":{"content":"later cd B"}}),
    ]);
    fixture.write(
        &b_dir,
        "b.jsonl",
        &[json!({"sessionId":"b","cwd":b,"type":"user","message":{"content":"origin B"}})],
    );
    fixture.write(
        &a_dir,
        "unknown.jsonl",
        &[json!({"sessionId":"unknown","type":"user","message":{"content":"no cwd"}})],
    );
    fixture.write(&a_dir, "changed/subagents/agent-unknown.jsonl", &[json!({"sessionId":"changed","isSidechain":true,"type":"assistant","message":{"content":"child without cwd"}})]);
    let parsed = parse_transcript(AgentCliKind::ClaudeCode, &changed)
        .unwrap()
        .unwrap();
    assert_eq!(parsed.native_origin_workdir, Some(a.clone()));
    for (cwd, expected) in [(&a, "changed"), (&b, "b")] {
        let groups = fixture.scan(std::slice::from_ref(cwd));
        assert_eq!(groups[0].records.len(), 1);
        assert_eq!(groups[0].records[0].summary.id, expected);
        assert!(!groups[0].complete);
        assert!(groups[0]
            .diagnostics
            .iter()
            .any(|message| message.contains("原生工作目录归属")));
    }
}

#[test]
fn short_exact_projects_keep_selected_workspace_when_native_cwd_is_missing() {
    let fixture = Fixture::new("short-unknown");
    let cwd = workdir(199, "");
    fixture.write(
        &directory(199, ""),
        "main.jsonl",
        &[json!({"sessionId":"main","type":"user","message":{"content":"short without cwd"}})],
    );
    let groups = fixture.scan(std::slice::from_ref(&cwd));
    assert!(groups[0].complete);
    assert_eq!(groups[0].records.len(), 1);
    assert_eq!(groups[0].records[0].summary.workdir, cwd.to_string_lossy());
}

#[cfg(unix)]
#[test]
fn candidate_discovery_deduplicates_aliases_and_rejects_symlink_projects() {
    use super::super::projects::{ProjectCandidateState, ProjectLookup, ProjectSelector};
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new("aliases");
    let cwd = fixture.0.join("workspace").join("a".repeat(180));
    fs::create_dir_all(&cwd).unwrap();
    let alias = fixture.0.join("workspace-alias");
    symlink(&cwd, &alias).unwrap();
    let raw = cwd.canonicalize().unwrap().to_string_lossy().to_string();
    let sanitized: String = raw
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() {
                value
            } else {
                '-'
            }
        })
        .collect();
    let name = format!("{}-bun-native", &sanitized[..200]);
    fixture.write(
        &name,
        "main.jsonl",
        &[json!({"sessionId":"main","cwd":cwd,"type":"user","message":{"content":"alias body"}})],
    );
    let outside = fixture.0.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("outside.jsonl"), json!({"sessionId":"outside","cwd":cwd,"type":"user","message":{"content":"must stay outside"}}).to_string()).unwrap();
    symlink(
        &outside,
        fixture
            .0
            .join("projects")
            .join(format!("{}-outside", &sanitized[..200])),
    )
    .unwrap();
    symlink(
        fixture.0.join("projects").join(&name),
        fixture
            .0
            .join("projects")
            .join(format!("{}-alias", &sanitized[..200])),
    )
    .unwrap();
    let groups = fixture.scan(std::slice::from_ref(&alias));
    assert_eq!(groups[0].records.len(), 1);
    assert_eq!(groups[0].records[0].summary.id, "main");
    let budget = SessionReadBudget::new(
        Instant::now() + Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
        1024,
    );
    let lookup = ProjectLookup::new(&fixture.0, &budget).unwrap();
    let candidates = lookup
        .discover(&[ProjectSelector::new(&alias)], &budget)
        .unwrap();
    assert_eq!(candidates[0].len(), 1);
    assert!(matches!(
        lookup
            .candidate(&outside, &ProjectSelector::new(&cwd), &budget)
            .unwrap(),
        ProjectCandidateState::Invalid
    ));
    assert!(
        std::path::Path::new(&groups[0].records[0].summary.workdir).ends_with("workspace-alias")
    );
}
