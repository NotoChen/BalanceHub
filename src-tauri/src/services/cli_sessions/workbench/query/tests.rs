use super::*;
use crate::services::agent_cli::contracts::EnvironmentPatch;
use serde_json::{json, Value};
use std::fs;

fn budget(bytes: u64) -> SessionReadBudget {
    SessionReadBudget::new(
        Instant::now() + Duration::from_secs(30),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        bytes,
    )
}
fn fixture(
    name: &str,
    count: usize,
) -> (
    PathBuf,
    AgentSessionSource,
    SessionHistorySource,
    AgentSessionWorkspace,
) {
    let root = std::env::temp_dir().join(format!(
        "balancehub-session-query-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let workdir = root.join("missing-project");
    let config = root.join("claude");
    let encoded: String = workdir
        .to_string_lossy()
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() {
                value
            } else {
                '-'
            }
        })
        .collect();
    let project = config.join("projects").join(encoded);
    fs::create_dir_all(&project).unwrap();
    for index in 0..count {
        let lines = [
            json!({"sessionId":format!("session-{index}"),"type":"user","message":{"content":"first request"},"timestamp":"2026-09-16T00:00:00Z"}),
            json!({"sessionId":format!("session-{index}"),"type":"assistant","message":{"content":[{"type":"text","text":"needle body"},{"type":"tool_use","name":"tool","input":{"value":"tool-only"}}]},"timestamp":"2026-09-16T01:00:00Z"}),
        ];
        fs::write(
            project.join(format!("session-{index}.jsonl")),
            lines
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
    }
    let source = AgentSessionSource {
        id: hash(&[&config.to_string_lossy()]),
        agent_kind: AgentCliKind::ClaudeCode,
        config_root: config.to_string_lossy().to_string(),
        available: true,
    };
    let native = SessionHistorySource {
        config_root: config,
        launch_environment: EnvironmentPatch::default(),
        resume_reason: None,
    };
    let workspace = AgentSessionWorkspace {
        id: hash(&[&workdir.to_string_lossy()]),
        path: workdir.to_string_lossy().to_string(),
        exists: false,
        is_home: false,
    };
    (root, source, native, workspace)
}
fn index_config(root: &std::path::Path) -> SessionIndexConfig {
    SessionIndexConfig {
        enabled: false,
        directory: root.join("derived-index"),
        max_size_bytes: 8 * 1024 * 1024,
    }
}
fn snapshot(rows: Vec<AgentSessionRow>, complete: bool) -> Snapshot {
    Snapshot {
        actor: "test".into(),
        consumer: "test".into(),
        scope_revision: "scope".into(),
        query_key: "query".into(),
        rows,
        parent_updates: HashMap::new(),
        states: Vec::new(),
        complete,
        progress: QueryProgress::default(),
        created_at: Instant::now(),
    }
}

fn claude_project(native: &SessionHistorySource, workspace: &AgentSessionWorkspace) -> PathBuf {
    let encoded: String = workspace
        .path
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() {
                value
            } else {
                '-'
            }
        })
        .collect();
    native.config_root.join("projects").join(encoded)
}

fn long_claude_workspace(suffix: &str) -> AgentSessionWorkspace {
    let prefix = if cfg!(target_os = "windows") {
        "C:\\fixture\\"
    } else {
        "/fixture/"
    };
    let path = format!(
        "{prefix}{}{suffix}",
        "a".repeat(240 - prefix.len() - suffix.len())
    );
    AgentSessionWorkspace {
        id: hash(&["workspace", &path]),
        path,
        exists: false,
        is_home: false,
    }
}

fn long_claude_record_path(native: &SessionHistorySource, suffix: &str, id: &str) -> PathBuf {
    let prefix = if cfg!(target_os = "windows") {
        "C--fixture-"
    } else {
        "-fixture-"
    };
    native
        .config_root
        .join("projects")
        .join(format!(
            "{prefix}{}-{suffix}",
            "a".repeat(200 - prefix.len())
        ))
        .join(format!("{id}.jsonl"))
}

fn write_long_claude_record(
    path: &std::path::Path,
    workspace: Option<&AgentSessionWorkspace>,
    id: &str,
    title: &str,
    size: usize,
) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut value = json!({"sessionId": id, "type":"user", "message":{"content":title}});
    if let Some(workspace) = workspace {
        value["cwd"] = json!(workspace.path);
    }
    let mut text = value.to_string();
    assert!(text.len() <= size);
    text.push_str(&" ".repeat(size - text.len()));
    fs::write(path, text).unwrap();
}

#[test]
fn long_claude_cached_origins_survive_both_workspace_orders_and_cursor_retries() {
    let (root, source, native, _) = fixture("long-origin-order", 0);
    let a = long_claude_workspace("A");
    let b = long_claude_workspace("B");
    write_long_claude_record(
        &long_claude_record_path(&native, "a-bun", "a"),
        Some(&a),
        "a",
        "A body",
        720,
    );
    write_long_claude_record(
        &long_claude_record_path(&native, "b-sdk", "b"),
        Some(&b),
        "b",
        "B body",
        720,
    );
    for workspaces in [[a.clone(), b.clone()], [b.clone(), a.clone()]] {
        let cache = Arc::new(Mutex::new(Default::default()));
        let mut saved = snapshot(Vec::new(), false);
        for pass in 0..2 {
            let limit = budget(4096);
            let outcome = with_read_budget(&limit, || {
                with_history_cache(&cache, || {
                    scan_source(
                        &source,
                        &native,
                        &workspaces,
                        "",
                        &index_config(&root),
                        &limit,
                        saved.progress.matches.clone(),
                    )
                })
            });
            assert!(!outcome.can_continue);
            assert_eq!(
                limit.bytes.load(Ordering::Relaxed),
                if pass == 0 { 1440 } else { 0 }
            );
            merge_source_outcomes(&mut saved, vec![outcome], AgentSessionRoleFilter::All);
            let first = page("long-order", &saved, 0, 1, &[], &[]);
            assert_eq!(first.total, Some(2));
            assert_eq!(first.next_cursor.as_deref(), Some("long-order:1"));
            assert_eq!(
                serde_json::to_value(&first).unwrap(),
                serde_json::to_value(page("long-order", &saved, 0, 1, &[], &[])).unwrap()
            );
            let second = page("long-order", &saved, 1, 1, &[], &[]);
            assert!(second.next_cursor.is_none());
            for row in first.items.iter().chain(&second.items) {
                let expected = if row.session.id == "a" {
                    &a
                } else {
                    assert_eq!(row.session.id, "b");
                    &b
                };
                assert_eq!(row.workspace_id, expected.id);
                assert_eq!(row.session.workdir, expected.path);
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn long_claude_raw_record_cache_advances_budget_and_revalidates_changed_origin() {
    let (root, source, native, _) = fixture("long-origin-budget", 0);
    let a = long_claude_workspace("A");
    let b = long_claude_workspace("B");
    let a_file = long_claude_record_path(&native, "a-bun", "a");
    let b_file = long_claude_record_path(&native, "b-sdk", "b");
    write_long_claude_record(&a_file, Some(&a), "a", "A body", 1080);
    write_long_claude_record(&b_file, Some(&b), "b", "B body", 1080);
    let cache = Arc::new(Mutex::new(Default::default()));
    let workspaces = [a.clone(), b.clone()];
    let mut saved = snapshot(Vec::new(), false);
    for pass in 0..2 {
        let limit = budget(1800);
        let outcome = with_read_budget(&limit, || {
            with_history_cache(&cache, || {
                scan_source(
                    &source,
                    &native,
                    &workspaces,
                    "",
                    &index_config(&root),
                    &limit,
                    saved.progress.matches.clone(),
                )
            })
        });
        assert_eq!(outcome.can_continue, pass == 0);
        merge_source_outcomes(&mut saved, vec![outcome], AgentSessionRoleFilter::All);
        let result = page("long-budget", &saved, 0, 50, &[], &[]);
        if pass == 0 {
            assert_eq!(result.items.len(), 1);
            assert_eq!(result.items[0].session.id, "a");
            assert_eq!(result.total, None);
            assert!(result.next_cursor.is_some());
        } else {
            assert_eq!(limit.bytes.load(Ordering::Relaxed), 1080);
            assert_eq!(result.total, Some(2));
            assert!(result.next_cursor.is_none());
            assert!(result
                .source_states
                .iter()
                .all(|state| state.state == AgentSessionSourceStatus::Complete));
        }
    }
    write_long_claude_record(&b_file, Some(&a), "b", "origin changed to A", 1081);
    let limit = budget(1800);
    let mut refreshed = snapshot(Vec::new(), false);
    let outcome = with_read_budget(&limit, || {
        with_history_cache(&cache, || {
            scan_source(
                &source,
                &native,
                &workspaces,
                "",
                &index_config(&root),
                &limit,
                refreshed.progress.matches.clone(),
            )
        })
    });
    assert!(!outcome.can_continue);
    assert_eq!(limit.bytes.load(Ordering::Relaxed), 1081);
    merge_source_outcomes(&mut refreshed, vec![outcome], AgentSessionRoleFilter::All);
    let result = page("long-changed", &refreshed, 0, 50, &[], &[]);
    assert_eq!(result.items.len(), 2);
    assert!(result
        .items
        .iter()
        .all(|row| row.workspace_id == a.id && row.session.workdir == a.path));
    assert_eq!(result.total, Some(2));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn long_claude_unknown_origin_is_partial_without_an_endless_cursor() {
    let (root, source, native, _) = fixture("long-origin-unknown", 0);
    let workspace = long_claude_workspace("A");
    write_long_claude_record(
        &long_claude_record_path(&native, "bun-unknown", "unknown"),
        None,
        "unknown",
        "unproven cwd",
        720,
    );
    let cache = Arc::new(Mutex::new(Default::default()));
    let mut saved = snapshot(Vec::new(), false);
    for pass in 0..2 {
        let limit = budget(1800);
        let outcome = with_read_budget(&limit, || {
            with_history_cache(&cache, || {
                scan_source(
                    &source,
                    &native,
                    std::slice::from_ref(&workspace),
                    "",
                    &index_config(&root),
                    &limit,
                    saved.progress.matches.clone(),
                )
            })
        });
        assert!(!outcome.can_continue);
        assert_eq!(
            limit.bytes.load(Ordering::Relaxed),
            if pass == 0 { 720 } else { 0 }
        );
        merge_source_outcomes(&mut saved, vec![outcome], AgentSessionRoleFilter::All);
        let result = page("long-unknown", &saved, 0, 50, &[], &[]);
        assert!(result.items.is_empty());
        assert_eq!(result.total, None);
        assert!(result.next_cursor.is_none());
        assert_eq!(
            result.source_states[0].state,
            AgentSessionSourceStatus::Partial
        );
        assert!(result.source_states[0]
            .message
            .as_deref()
            .unwrap()
            .contains("原生工作目录归属"));
    }
    fs::remove_dir_all(root).unwrap();
}

fn write_claude_record(path: &std::path::Path, native_id: &str, text: &str, timestamp: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        json!({
            "sessionId": native_id,
            "type": "user",
            "message": { "content": text },
            "timestamp": timestamp,
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn more_than_one_hundred_native_sessions_and_fifty_body_matches_are_paged_without_caps() {
    let (root, source, native, workspace) = fixture("pagination", 155);
    let limit = budget(16 * 1024 * 1024);
    let outcome = with_read_budget(&limit, || {
        scan_source(
            &source,
            &native,
            std::slice::from_ref(&workspace),
            "needle",
            &index_config(&root),
            &limit,
            Arc::new(Mutex::new(HashMap::new())),
        )
    });
    assert_eq!(outcome.references.len(), 155);
    assert_eq!(outcome.matched.len(), 155);
    assert_eq!(outcome.states[0].state, AgentSessionSourceStatus::Complete);
    assert!(
        outcome
            .references
            .iter()
            .all(|entry| !entry.row.session.can_resume),
        "missing workdir affects resume only"
    );
    let mut data = snapshot(Vec::new(), false);
    merge_source_outcomes(&mut data, vec![outcome], AgentSessionRoleFilter::All);
    let first = page(
        "snapshot",
        &data,
        0,
        1,
        std::slice::from_ref(&source),
        std::slice::from_ref(&workspace),
    );
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.loaded_count, 155);
    assert_eq!(
        first.agent_counts,
        vec![AgentSessionCount {
            agent_kind: AgentCliKind::ClaudeCode,
            loaded_count: 155,
            total: Some(155),
        }]
    );
    let wire = serde_json::to_value(&first).unwrap();
    assert_eq!(
        wire["agentCounts"],
        json!([
            { "agentKind": "claudeCode", "loadedCount": 155, "total": 155 }
        ])
    );
    assert!(wire.get("agent_counts").is_none());
    let mut delivered = HashSet::new();
    for offset in [0, 50, 100, 150] {
        let page = page(
            "snapshot",
            &data,
            offset,
            50,
            std::slice::from_ref(&source),
            std::slice::from_ref(&workspace),
        );
        for row in page.items {
            assert!(delivered.insert(row.session_ref));
        }
        assert_eq!(page.total, Some(155));
        assert_eq!(page.agent_counts, first.agent_counts);
        assert_eq!(page.next_cursor.is_some(), offset < 150);
    }
    assert_eq!(delivered.len(), 155);
    let no_tools = with_read_budget(&limit, || {
        scan_source(
            &source,
            &native,
            &[workspace],
            "tool-only",
            &index_config(&root),
            &limit,
            Arc::new(Mutex::new(HashMap::new())),
        )
    });
    assert!(no_tools.matched.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn agent_counts_include_main_and_subagents_once_across_duplicate_scans_and_cursor_retries() {
    let (root, source, native, workspace) = fixture("count-dedup", 2);
    let child = claude_project(&native, &workspace).join("session-0/subagents/child.jsonl");
    write_claude_record(&child, "child", "subagent request", "2026-09-17T00:00:00Z");
    let limit = budget(1024 * 1024);
    let mut outcome = with_read_budget(&limit, || {
        scan_source(
            &source,
            &native,
            std::slice::from_ref(&workspace),
            "",
            &index_config(&root),
            &limit,
            Arc::new(Mutex::new(HashMap::new())),
        )
    });
    assert_eq!(outcome.references.len(), 3);
    outcome.references.extend(outcome.references.clone());
    outcome.states[0].loaded_count = outcome.references.len();
    let repeated = SourceOutcome {
        references: outcome.references.clone(),
        matched: outcome.matched.clone(),
        states: outcome.states.clone(),
        can_continue: outcome.can_continue,
    };
    let mut saved = snapshot(Vec::new(), false);
    merge_source_outcomes(&mut saved, vec![outcome], AgentSessionRoleFilter::All);
    assert_eq!(
        saved
            .rows
            .iter()
            .filter(|row| row.role == AgentSessionRole::Main)
            .count(),
        2
    );
    assert_eq!(
        saved
            .rows
            .iter()
            .filter(|row| row.role == AgentSessionRole::Subagent)
            .count(),
        1
    );
    let first = page(
        "counts",
        &saved,
        0,
        1,
        std::slice::from_ref(&source),
        std::slice::from_ref(&workspace),
    );
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.source_states[0].loaded_count, 6);
    assert_eq!(first.agent_counts[0].loaded_count, 3);
    assert_eq!(first.agent_counts[0].total, Some(3));

    merge_source_outcomes(&mut saved, vec![repeated], AgentSessionRoleFilter::All);
    let next = page(
        "counts",
        &saved,
        1,
        1,
        std::slice::from_ref(&source),
        std::slice::from_ref(&workspace),
    );
    assert_eq!(saved.rows.len(), 3);
    assert_eq!(next.agent_counts, first.agent_counts);
    assert_ne!(next.items[0].session_ref, first.items[0].session_ref);
    assert_eq!(
        serde_json::to_value(&next).unwrap(),
        serde_json::to_value(page(
            "counts",
            &saved,
            1,
            1,
            std::slice::from_ref(&source),
            std::slice::from_ref(&workspace)
        ))
        .unwrap(),
        "retrying the same cursor must not accumulate counts",
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn agent_counts_require_every_expected_source_workspace_pair_and_keep_complete_agents_exact() {
    let sources = [
        count_source("codex", AgentCliKind::Codex),
        count_source("claude-main", AgentCliKind::ClaudeCode),
        count_source("claude-other", AgentCliKind::ClaudeCode),
        count_source("gemini", AgentCliKind::Gemini),
    ];
    let workspaces = [count_workspace("home"), count_workspace("project")];
    let mut saved = snapshot(Vec::new(), false);
    saved.states = sources
        .iter()
        .flat_map(|source| {
            workspaces
                .iter()
                .map(|workspace| count_state(source, workspace, AgentSessionSourceStatus::Complete))
        })
        .collect();
    for incomplete in [
        AgentSessionSourceStatus::Partial,
        AgentSessionSourceStatus::Unavailable,
        AgentSessionSourceStatus::Unsupported,
        AgentSessionSourceStatus::Cancelled,
    ] {
        saved.states.last_mut().unwrap().state = incomplete;
        let counts = agent_counts(&saved, &sources, &workspaces);
        assert_eq!(
            counts,
            vec![
                AgentSessionCount {
                    agent_kind: AgentCliKind::Codex,
                    loaded_count: 0,
                    total: Some(0)
                },
                AgentSessionCount {
                    agent_kind: AgentCliKind::ClaudeCode,
                    loaded_count: 0,
                    total: Some(0)
                },
                AgentSessionCount {
                    agent_kind: AgentCliKind::Gemini,
                    loaded_count: 0,
                    total: None
                },
            ]
        );
    }
    saved
        .states
        .retain(|state| state.source_id != "claude-other" || state.workspace_id != "project");
    let counts = agent_counts(&saved, &sources, &workspaces);
    assert_eq!(counts[0].total, Some(0));
    assert_eq!(
        counts[1].total, None,
        "a missing workspace status in another source of the same Agent is incomplete"
    );
    assert_eq!(counts[2].total, None);
    assert!(counts
        .iter()
        .all(|count| count.agent_kind != AgentCliKind::Grok));
    assert!(
        agent_counts(&saved, &[], &workspaces).is_empty(),
        "unselected Agents do not gain invented zero counts"
    );
    assert!(
        agent_counts(&saved, &sources, &[])
            .iter()
            .all(|count| count.total.is_none()),
        "an empty expected workspace set cannot prove exact zero"
    );
    saved.states.clear();
    assert!(agent_counts(&saved, &sources, &workspaces)
        .iter()
        .all(|count| count.total.is_none()));
}

fn count_source(id: &str, kind: AgentCliKind) -> AgentSessionSource {
    AgentSessionSource {
        id: id.into(),
        agent_kind: kind,
        config_root: format!("/fixture/{id}"),
        available: true,
    }
}

fn count_workspace(id: &str) -> AgentSessionWorkspace {
    AgentSessionWorkspace {
        id: id.into(),
        path: format!("/fixture/{id}"),
        exists: true,
        is_home: id == "home",
    }
}

fn count_state(
    source: &AgentSessionSource,
    workspace: &AgentSessionWorkspace,
    status: AgentSessionSourceStatus,
) -> AgentSessionSourceState {
    AgentSessionSourceState {
        source_id: source.id.clone(),
        workspace_id: workspace.id.clone(),
        state: status,
        loaded_count: 999,
        message: None,
        index_state: CliSessionIndexState::Ready,
    }
}

#[test]
fn budget_continuation_reuses_completed_summaries_and_progresses_to_the_remaining_files() {
    let (root, source, native, workspace) = fixture("continue", 12);
    let cache = Arc::new(Mutex::new(Default::default()));
    let matches = Arc::new(Mutex::new(HashMap::new()));
    let mut rows = Vec::new();
    let mut complete = false;
    for _ in 0..20 {
        let limit = budget(1800);
        let outcome = with_read_budget(&limit, || {
            with_history_cache(&cache, || {
                scan_source(
                    &source,
                    &native,
                    std::slice::from_ref(&workspace),
                    "",
                    &index_config(&root),
                    &limit,
                    matches.clone(),
                )
            })
        });
        append_batch(
            &mut rows,
            outcome
                .references
                .into_iter()
                .map(|entry| entry.row)
                .collect(),
        );
        complete = outcome
            .states
            .iter()
            .all(|state| state.state == AgentSessionSourceStatus::Complete);
        if complete {
            break;
        }
        assert!(outcome.can_continue);
        assert_eq!(
            page("partial", &snapshot(rows.clone(), false), 0, 50, &[], &[]).total,
            None
        );
    }
    assert!(
        complete,
        "each retry must advance instead of rereading the same prefix"
    );
    assert_eq!(rows.len(), 12);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn oversized_files_keep_proven_metadata_and_continuation_terminates_after_healthy_files() {
    let (root, source, native, workspace) = fixture("oversized-continuation", 12);
    let project = claude_project(&native, &workspace);
    let oversized = project.join("session-0.jsonl");
    let prefix = json!({
        "sessionId": "session-0", "type": "user",
        "message": { "content": "proven-prefix-title" },
        "timestamp": "2026-09-16T00:00:00Z",
    })
    .to_string();
    let tail = json!({
        "sessionId": "session-0", "type": "assistant",
        "message": { "content": "unread-tail-only ".repeat(600) },
        "timestamp": "2026-09-17T00:00:00Z",
    })
    .to_string();
    fs::write(&oversized, format!("{prefix}\n{tail}")).unwrap();
    // A file name and a native ID beyond the allowed prefix are not evidence.
    for index in 0..6 {
        fs::write(project.join(format!("not-a-proven-session-{index}.jsonl")), format!(
            "{}\n{}\n{}",
            json!({"type":"metadata"}),
            json!({"padding":"x".repeat(3000)}),
            json!({"sessionId":format!("late-unverified-id-{index}"),"type":"user","message":{"content":"late"}}),
        )).unwrap();
    }
    let cache = Arc::new(Mutex::new(Default::default()));
    let matches = Arc::new(Mutex::new(HashMap::new()));
    let mut saved = snapshot(Vec::new(), false);
    let mut stopped = false;
    let mut final_references = Vec::new();
    for _ in 0..20 {
        let limit = budget(1800);
        let outcome = with_read_budget(&limit, || {
            with_history_cache(&cache, || {
                scan_source(
                    &source,
                    &native,
                    std::slice::from_ref(&workspace),
                    "",
                    &index_config(&root),
                    &limit,
                    matches.clone(),
                )
            })
        });
        final_references =
            merge_source_outcomes(&mut saved, vec![outcome], AgentSessionRoleFilter::All);
        let end = page("oversized", &saved, saved.rows.len(), 50, &[], &[]);
        if end.next_cursor.is_none() {
            stopped = true;
            break;
        }
    }
    assert!(
        stopped,
        "a terminal file limit must not create an endless cursor"
    );
    assert_eq!(
        saved.rows.len(),
        12,
        "eleven healthy records and one proven prefix remain listable"
    );
    assert_eq!(saved.states[0].state, AgentSessionSourceStatus::Partial);
    assert!(saved.states[0]
        .message
        .as_deref()
        .unwrap()
        .contains("单文件超过读取上限"));
    assert!(saved
        .rows
        .iter()
        .all(|row| !row.session.id.starts_with("late-unverified-id")
            && !row.session.id.starts_with("not-a-proven-session")));
    let limited = final_references
        .iter()
        .find(|entry| entry.row.session.id == "session-0")
        .unwrap();
    assert_eq!(limited.row.session.title, "proven-prefix-title");
    assert_eq!(limited.row.session.workdir, workspace.path);
    assert_eq!(limited.row.role, AgentSessionRole::Main);
    assert_eq!(limited.row.session.updated_at, None);
    assert!(!limited.row.session.can_resume);
    assert!(limited.record.content_unavailable_reason.is_some());
    let adapter = history_adapter(AgentCliKind::ClaudeCode).unwrap();
    assert!((adapter.detail)(
        &limited.record,
        crate::services::cli_sessions::DETAIL_READ_LIMITS
    )
    .is_err());
    let end = page("oversized", &saved, 0, 50, &[], &[]);
    assert_eq!(end.total, None);
    assert!(end.next_cursor.is_none());

    let limit = budget(1800);
    let hidden = with_read_budget(&limit, || {
        with_history_cache(&cache, || {
            scan_source(
                &source,
                &native,
                std::slice::from_ref(&workspace),
                "unread-tail-only",
                &index_config(&root),
                &limit,
                Arc::new(Mutex::new(HashMap::new())),
            )
        })
    });
    assert!(hidden.matched.is_empty());
    assert_eq!(hidden.states[0].state, AgentSessionSourceStatus::Partial);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn two_files_each_below_the_fixed_limit_resume_instead_of_becoming_terminal_oversized_files() {
    let (root, source, native, workspace) = fixture("sixty-percent-files", 2);
    let project = claude_project(&native, &workspace);
    for index in 0..2 {
        let mut text = json!({
            "sessionId": format!("session-{index}"), "type":"user",
            "message": {"content":"healthy sixty percent file"},
        })
        .to_string();
        text.push('\n');
        text.push_str(&" ".repeat(1080 - text.len()));
        assert_eq!(text.len(), 1800 * 3 / 5);
        fs::write(project.join(format!("session-{index}.jsonl")), text).unwrap();
    }
    let cache = Arc::new(Mutex::new(Default::default()));
    let mut saved = snapshot(Vec::new(), false);
    for pass in 0..2 {
        let limit = budget(1800);
        let outcome = with_read_budget(&limit, || {
            with_history_cache(&cache, || {
                scan_source(
                    &source,
                    &native,
                    std::slice::from_ref(&workspace),
                    "",
                    &index_config(&root),
                    &limit,
                    saved.progress.matches.clone(),
                )
            })
        });
        assert_eq!(outcome.can_continue, pass == 0);
        assert!(outcome.states.iter().all(|state| !state
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("单文件超过读取上限")));
        merge_source_outcomes(&mut saved, vec![outcome], AgentSessionRoleFilter::All);
    }
    assert_eq!(saved.rows.len(), 2);
    let page = page("two-files", &saved, 0, 50, &[], &[]);
    assert_eq!(page.total, Some(2));
    assert!(page.next_cursor.is_none());
    fs::remove_dir_all(root).unwrap();
}

fn assert_source_metadata_continuation(kind: AgentCliKind) {
    let (root, mut source, native, workspace) =
        fixture(&format!("{}-metadata-budget", kind.key()), 0);
    source.agent_kind = kind;
    let padded = |mut text: String, size: usize| {
        assert!(text.len() <= size);
        text.push_str(&" ".repeat(size - text.len()));
        text
    };
    match kind {
        AgentCliKind::Gemini => {
            let project = native.config_root.join("tmp/mapped-project");
            fs::create_dir_all(project.join("chats")).unwrap();
            fs::write(
                native.config_root.join("projects.json"),
                padded(
                    json!({"projects": {workspace.path.clone(): "mapped-project"}}).to_string(),
                    720,
                ),
            )
            .unwrap();
            fs::write(
                project.join(".project_root"),
                padded(workspace.path.clone(), 720),
            )
            .unwrap();
            fs::write(
                project.join("chats/native-session.jsonl"),
                padded(
                    [
                        json!({"sessionId":"native-session","startTime":"2026-09-17T00:00:00Z"})
                            .to_string(),
                        json!({"id":"user-1","type":"user","content":"healthy mapped session"})
                            .to_string(),
                    ]
                    .join("\n"),
                    1440,
                ),
            )
            .unwrap();
        }
        AgentCliKind::Grok => {
            let project = native.config_root.join("sessions/mapped-project");
            fs::create_dir_all(project.join("native-session")).unwrap();
            fs::write(project.join(".cwd"), padded(workspace.path.clone(), 720)).unwrap();
            fs::write(
                project.join("native-session/summary.json"),
                padded(
                    json!({
                        "info": {"id":"native-session", "cwd":workspace.path},
                        "generated_title":"healthy mapped session", "num_messages":1,
                    })
                    .to_string(),
                    1440,
                ),
            )
            .unwrap();
        }
        _ => unreachable!("this fixture exercises the two source metadata formats"),
    }
    let cache = Arc::new(Mutex::new(Default::default()));
    let mut saved = snapshot(Vec::new(), false);
    for pass in 0..2 {
        let limit = budget(1800);
        let outcome = with_read_budget(&limit, || {
            with_history_cache(&cache, || {
                scan_source(
                    &source,
                    &native,
                    std::slice::from_ref(&workspace),
                    "",
                    &index_config(&root),
                    &limit,
                    saved.progress.matches.clone(),
                )
            })
        });
        assert_eq!(outcome.can_continue, pass == 0, "{kind:?}");
        merge_source_outcomes(&mut saved, vec![outcome], AgentSessionRoleFilter::All);
        let result = page("metadata", &saved, 0, 50, &[], &[]);
        if pass == 0 {
            assert!(result.items.is_empty(), "{kind:?}");
            assert_eq!(result.next_cursor.as_deref(), Some("metadata:0"));
            assert_eq!(result.total, None);
        } else {
            assert_eq!(
                limit.bytes.load(Ordering::Relaxed),
                1440,
                "only the unfinished session should consume I/O bytes on continuation: {kind:?}"
            );
            assert_eq!(result.items.len(), 1);
            assert_eq!(result.items[0].session.id, "native-session");
            assert_eq!(result.items[0].session.title, "healthy mapped session");
            assert_eq!(result.total, Some(1));
            assert!(result.next_cursor.is_none());
            assert_eq!(
                result.source_states[0].state,
                AgentSessionSourceStatus::Complete
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn gemini_project_metadata_cache_allows_a_valid_session_to_finish_on_the_next_page() {
    assert_source_metadata_continuation(AgentCliKind::Gemini);
}

#[test]
fn grok_cwd_metadata_cache_allows_a_valid_summary_to_finish_on_the_next_page() {
    assert_source_metadata_continuation(AgentCliKind::Grok);
}

#[test]
fn maintenance_budget_expiry_keeps_native_results_and_diagnostics_without_a_retry_cursor() {
    let (root, source, native, workspace) = fixture("maintenance-budget", 3);
    let mut derived = index_config(&root);
    derived.enabled = true;
    let seeded_budget = budget(4 * 1024 * 1024);
    let seeded = with_read_budget(&seeded_budget, || {
        scan_source(
            &source,
            &native,
            std::slice::from_ref(&workspace),
            "needle",
            &derived,
            &seeded_budget,
            Arc::new(Mutex::new(HashMap::new())),
        )
    });
    assert_eq!(seeded.matched.len(), 3);
    assert_eq!(seeded.states[0].index_state, CliSessionIndexState::Ready);
    let project = claude_project(&native, &workspace);
    fs::write(project.join("corrupt.jsonl"), "not a native JSON event").unwrap();
    let native_bytes: u64 = fs::read_dir(&project)
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
    // Native summaries finish; the first capacity candidate then exhausts the
    // shared byte budget. This drives the real scan -> finish -> merge -> page.
    derived.max_size_bytes = 1;
    let limit = budget(native_bytes + 1);
    let outcome = with_read_budget(&limit, || {
        scan_source(
            &source,
            &native,
            std::slice::from_ref(&workspace),
            "first request",
            &derived,
            &limit,
            Arc::new(Mutex::new(HashMap::new())),
        )
    });
    assert!(!outcome.can_continue);
    assert_eq!(outcome.references.len(), 3);
    assert_eq!(outcome.matched.len(), 3);
    assert_eq!(outcome.states[0].state, AgentSessionSourceStatus::Partial);
    assert_eq!(
        outcome.states[0].index_state,
        CliSessionIndexState::Fallback
    );
    let message = outcome.states[0].message.as_deref().unwrap();
    assert!(message.contains("没有可解析的原生事件"));
    assert!(message.contains("索引容量维护未完成"));
    assert!(message.contains("字节预算"));
    let (healthy_root, healthy_source, healthy_native, healthy_workspace) =
        fixture("maintenance-healthy-source", 2);
    let healthy_budget = budget(1024 * 1024);
    let healthy = with_read_budget(&healthy_budget, || {
        scan_source(
            &healthy_source,
            &healthy_native,
            &[healthy_workspace],
            "",
            &index_config(&healthy_root),
            &healthy_budget,
            Arc::new(Mutex::new(HashMap::new())),
        )
    });
    let mut saved = snapshot(Vec::new(), false);
    merge_source_outcomes(
        &mut saved,
        vec![outcome, healthy],
        AgentSessionRoleFilter::All,
    );
    let page = page("maintenance", &saved, 0, 50, &[], &[]);
    assert_eq!(page.items.len(), 5);
    assert_eq!(page.total, None);
    assert!(page.next_cursor.is_none());
    assert_eq!(
        page.source_states[1].state,
        AgentSessionSourceStatus::Complete
    );
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(healthy_root).unwrap();
}

#[test]
fn a_later_batch_with_newer_timestamps_does_not_repeat_or_hide_rows_behind_a_cursor() {
    let (root, source, native, workspace) = fixture("late-newer", 3);
    let limit = budget(1024 * 1024);
    let outcome = with_read_budget(&limit, || {
        scan_source(
            &source,
            &native,
            &[workspace],
            "",
            &index_config(&root),
            &limit,
            Arc::new(Mutex::new(HashMap::new())),
        )
    });
    let mut rows: Vec<_> = outcome
        .references
        .into_iter()
        .map(|entry| entry.row)
        .collect();
    let mut latest = rows.pop().unwrap();
    latest.session.updated_at = Some("2026-09-17T12:00:00Z".into());
    let mut saved = snapshot(rows.clone(), false);
    let first = page("snapshot", &saved, 0, 1, &[], &[]);
    append_batch(&mut saved.rows, vec![latest.clone(), rows[0].clone()]);
    let remaining = page("snapshot", &saved, 1, 50, &[], &[]);
    let all: HashSet<_> = first
        .items
        .iter()
        .chain(remaining.items.iter())
        .map(|row| &row.session_ref)
        .collect();
    assert_eq!(all.len(), 3);
    assert_eq!(saved.rows.last().unwrap().session_ref, latest.session_ref);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_references_separate_libraries_and_stay_independent_of_window_or_query_revision() {
    let (root, mut source, native, workspace) = fixture("identity", 1);
    let limit = budget(1024 * 1024);
    let outcome = with_read_budget(&limit, || {
        scan_source(
            &source,
            &native,
            std::slice::from_ref(&workspace),
            "",
            &index_config(&root),
            &limit,
            Arc::new(Mutex::new(HashMap::new())),
        )
    });
    let entry = &outcome.references[0];
    assert_eq!(
        entry.row.session_ref,
        row_for_record(&source, &native, &workspace, &entry.record).session_ref
    );
    source.id = "another-library".into();
    assert_ne!(
        entry.row.session_ref,
        row_for_record(&source, &native, &workspace, &entry.record).session_ref
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn nested_subagents_link_to_unique_parents_outside_the_page_role_and_search_results() {
    let (root, source, native, workspace) = fixture("nested-parents", 1);
    let project = claude_project(&native, &workspace);
    write_claude_record(
        &project.join("session-0/subagents/agent-parent.jsonl"),
        "session-0",
        "parent branch",
        "2026-09-17T00:00:00Z",
    );
    write_claude_record(
        &project.join("agent-parent/subagents/agent-leaf.jsonl"),
        "agent-parent",
        "leaf-only-marker",
        "2026-09-17T01:00:00Z",
    );
    for (text, filter, expected_count) in [
        ("", AgentSessionRoleFilter::All, 3),
        ("", AgentSessionRoleFilter::Subagent, 2),
        ("leaf-only-marker", AgentSessionRoleFilter::Subagent, 1),
    ] {
        let limit = budget(1024 * 1024);
        let outcome = with_read_budget(&limit, || {
            scan_source(
                &source,
                &native,
                std::slice::from_ref(&workspace),
                text,
                &index_config(&root),
                &limit,
                Arc::new(Mutex::new(HashMap::new())),
            )
        });
        let mut saved = snapshot(Vec::new(), false);
        let references = merge_source_outcomes(&mut saved, vec![outcome], filter);
        let parent = references
            .iter()
            .find(|entry| entry.row.session.id == "agent-parent")
            .unwrap();
        let main = references
            .iter()
            .find(|entry| entry.row.session.id == "session-0")
            .unwrap();
        assert_eq!(parent.row.role, AgentSessionRole::Subagent);
        assert!(matches!(&parent.row.parent,
            AgentSessionParent::Known { parent_ref: Some(parent_ref), .. }
                if parent_ref == &main.row.session_ref
        ));
        let first = page("nested", &saved, 0, 1, &[], &[]);
        assert_eq!(saved.rows.len(), expected_count);
        assert_eq!(first.items[0].session.id, "agent-leaf");
        assert!(matches!(&first.items[0].parent,
            AgentSessionParent::Known { native_id, parent_ref: Some(parent_ref) }
                if native_id == "agent-parent" && parent_ref == &parent.row.session_ref
        ));
        assert!(!first
            .items
            .iter()
            .any(|row| row.session_ref == parent.row.session_ref));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn ambiguous_parent_ids_never_cross_sources_or_workspaces() {
    let (root, source, native, workspace) = fixture("ambiguous-parents", 1);
    let project = claude_project(&native, &workspace);
    write_claude_record(
        &project.join("session-0/subagents/child.jsonl"),
        "session-0",
        "child",
        "2026-09-17T00:00:00Z",
    );
    // A distinct child record with the same native ID makes this scope ambiguous.
    write_claude_record(
        &project.join("anchor/subagents/session-0.jsonl"),
        "anchor",
        "duplicate parent ID",
        "2026-09-17T00:00:00Z",
    );
    let other_workspace = AgentSessionWorkspace {
        id: "other-workspace".into(),
        path: root.join("other-project").to_string_lossy().to_string(),
        exists: false,
        is_home: false,
    };
    write_claude_record(
        &claude_project(&native, &other_workspace).join("session-0/subagents/other-child.jsonl"),
        "session-0",
        "other workspace child",
        "2026-09-17T00:00:00Z",
    );
    let (other_root, other_source, other_native, _) = fixture("separate-parent-source", 0);
    let other_project = claude_project(&other_native, &workspace);
    write_claude_record(
        &other_project.join("session-0.jsonl"),
        "session-0",
        "unique other source parent",
        "2026-09-17T00:00:00Z",
    );
    write_claude_record(
        &other_project.join("session-0/subagents/child.jsonl"),
        "session-0",
        "other source child",
        "2026-09-17T00:00:00Z",
    );
    let limit = budget(1024 * 1024);
    let own = with_read_budget(&limit, || {
        scan_source(
            &source,
            &native,
            &[workspace.clone(), other_workspace.clone()],
            "",
            &index_config(&root),
            &limit,
            Arc::new(Mutex::new(HashMap::new())),
        )
    });
    let other_limit = budget(1024 * 1024);
    let other = with_read_budget(&other_limit, || {
        scan_source(
            &other_source,
            &other_native,
            std::slice::from_ref(&workspace),
            "",
            &index_config(&other_root),
            &other_limit,
            Arc::new(Mutex::new(HashMap::new())),
        )
    });
    let mut saved = snapshot(Vec::new(), false);
    let references = merge_source_outcomes(
        &mut saved,
        vec![own, other],
        AgentSessionRoleFilter::Subagent,
    );
    let own_child = saved
        .rows
        .iter()
        .find(|row| {
            row.source_id == source.id
                && row.workspace_id == workspace.id
                && row.session.id == "child"
        })
        .unwrap();
    assert!(matches!(&own_child.parent,
        AgentSessionParent::Known { native_id, parent_ref: None } if native_id == "session-0"
    ));
    let other_workspace_child = saved
        .rows
        .iter()
        .find(|row| row.workspace_id == other_workspace.id)
        .unwrap();
    assert!(matches!(&other_workspace_child.parent,
        AgentSessionParent::Known { native_id, parent_ref: None } if native_id == "session-0"
    ));
    let other_parent = references
        .iter()
        .find(|entry| entry.row.source_id == other_source.id && entry.row.session.id == "session-0")
        .unwrap();
    let other_child = saved
        .rows
        .iter()
        .find(|row| row.source_id == other_source.id && row.session.id == "child")
        .unwrap();
    assert!(matches!(&other_child.parent,
        AgentSessionParent::Known { parent_ref: Some(parent_ref), .. }
            if parent_ref == &other_parent.row.session_ref
    ));
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(other_root).unwrap();
}

#[test]
fn later_parent_discovery_and_ambiguity_are_serialized_as_updates_for_delivered_rows() {
    let (root, source, native, workspace) = fixture("late-parent-link", 0);
    let project = claude_project(&native, &workspace);
    write_claude_record(
        &project.join("parent/subagents/child.jsonl"),
        "parent",
        "early child",
        "2026-09-17T00:00:00Z",
    );
    let mut saved = snapshot(Vec::new(), false);
    let mut child_ref = None;
    for pass in 0..3 {
        if pass == 1 {
            write_claude_record(
                &project.join("parent.jsonl"),
                "parent",
                "later discovered parent",
                "2026-09-16T00:00:00Z",
            );
        } else if pass == 2 {
            write_claude_record(
                &project.join("ancestor/subagents/parent.jsonl"),
                "ancestor",
                "ambiguous parent",
                "2026-09-16T00:00:00Z",
            );
        }
        let limit = budget(1024 * 1024);
        let mut outcome = with_read_budget(&limit, || {
            scan_source(
                &source,
                &native,
                std::slice::from_ref(&workspace),
                "early child",
                &index_config(&root),
                &limit,
                Arc::new(Mutex::new(HashMap::new())),
            )
        });
        // Represent three native discovery batches. The search returns only the
        // child, so later pages have no new items and must carry corrections.
        outcome.can_continue = pass < 2;
        for state in &mut outcome.states {
            state.state = if pass < 2 {
                AgentSessionSourceStatus::Partial
            } else {
                AgentSessionSourceStatus::Complete
            };
        }
        let references =
            merge_source_outcomes(&mut saved, vec![outcome], AgentSessionRoleFilter::All);
        let first = &saved.rows[0];
        assert_eq!(first.session.id, "child");
        if let Some(reference) = &child_ref {
            assert_eq!(&first.session_ref, reference);
        } else {
            child_ref = Some(first.session_ref.clone());
        }
        let AgentSessionParent::Known {
            native_id,
            parent_ref,
        } = &first.parent
        else {
            panic!("the native parent relationship must remain known");
        };
        assert_eq!(native_id, "parent");
        assert_eq!(parent_ref.is_some(), pass == 1);
        assert_eq!(saved.rows.len(), 1);
        let offset = usize::from(pass > 0);
        let payload =
            serde_json::to_value(page("late-parent", &saved, offset, 50, &[], &[])).unwrap();
        assert_eq!(payload["loadedCount"], 1);
        assert_eq!(
            payload["items"].as_array().unwrap().len(),
            usize::from(pass == 0)
        );
        assert_eq!(
            payload["nextCursor"],
            json!(if pass < 2 {
                Some("late-parent:1")
            } else {
                None
            })
        );
        assert!(payload.get("parent_updates").is_none());
        if pass == 0 {
            assert_eq!(payload["parentUpdates"], json!([]));
            assert_eq!(payload["items"][0]["parent"]["parentRef"], Value::Null);
        } else {
            let expected_parent_ref = (pass == 1).then(|| {
                references
                    .iter()
                    .find(|entry| {
                        entry.row.role == AgentSessionRole::Main && entry.row.session.id == "parent"
                    })
                    .unwrap()
                    .row
                    .session_ref
                    .clone()
            });
            assert_eq!(
                payload["parentUpdates"],
                json!([{
                    "sessionRef": child_ref.as_deref().unwrap(),
                    "parent": { "kind": "known", "nativeId": "parent", "parentRef": expected_parent_ref },
                }])
            );
        }
        assert_eq!(
            serde_json::to_value(page("late-parent", &saved, offset, 50, &[], &[])).unwrap(),
            payload,
            "retrying the same cursor must return the same latest correction",
        );
        assert!(
            page("late-parent", &saved, 0, 50, &[], &[])
                .parent_updates
                .is_empty(),
            "current-page items already contain their latest parent; do not resend them as updates"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn oversized_codex_rollout_keeps_native_sqlite_metadata_and_has_no_endless_cursor() {
    let root = std::env::temp_dir().join(format!(
        "balancehub-codex-oversized-rollout-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let config_root = root.join("codex");
    let workdir = root.join("project");
    fs::create_dir_all(config_root.join("sessions")).unwrap();
    fs::create_dir_all(&workdir).unwrap();
    fs::write(config_root.join("sessions/large.jsonl"), [
        json!({"type":"session_meta","payload":{"source":"cli"}}).to_string(),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"unread-body-marker ".repeat(1000)}}).to_string(),
    ].join("\n")).unwrap();
    let connection = rusqlite::Connection::open(config_root.join("state_5.sqlite")).unwrap();
    connection.execute_batch("CREATE TABLE threads (id TEXT, cwd TEXT, title TEXT, model TEXT, updated_at_ms INTEGER, rollout_path TEXT, source TEXT)").unwrap();
    connection.execute(
        "INSERT INTO threads VALUES ('large-native-id', ?1, 'native-index-title', 'fixture-model', 1786000005000, 'sessions/large.jsonl', 'cli')",
        [workdir.to_string_lossy().as_ref()],
    ).unwrap();
    drop(connection);
    let source = AgentSessionSource {
        id: hash(&[&config_root.to_string_lossy()]),
        agent_kind: AgentCliKind::Codex,
        config_root: config_root.to_string_lossy().to_string(),
        available: true,
    };
    let native = SessionHistorySource {
        config_root,
        launch_environment: EnvironmentPatch::default(),
        resume_reason: None,
    };
    let workspace = AgentSessionWorkspace {
        id: hash(&[&workdir.to_string_lossy()]),
        path: workdir.to_string_lossy().to_string(),
        exists: true,
        is_home: false,
    };
    let cache = Arc::new(Mutex::new(Default::default()));
    for text in [
        "native-index-title",
        "unread-body-marker",
        "native-index-title",
    ] {
        let limit = budget(4096);
        let outcome = with_read_budget(&limit, || {
            with_history_cache(&cache, || {
                scan_source(
                    &source,
                    &native,
                    std::slice::from_ref(&workspace),
                    text,
                    &index_config(&root),
                    &limit,
                    Arc::new(Mutex::new(HashMap::new())),
                )
            })
        });
        assert!(limit.check().is_ok());
        assert!(!outcome.can_continue);
        assert_eq!(outcome.references.len(), 1);
        let row = &outcome.references[0].row;
        assert_eq!(row.session.id, "large-native-id");
        assert_eq!(row.session.title, "native-index-title");
        assert_eq!(row.session.workdir, workspace.path);
        assert_eq!(row.role, AgentSessionRole::Main);
        assert!(!row.session.can_resume);
        let mut saved = snapshot(Vec::new(), false);
        merge_source_outcomes(&mut saved, vec![outcome], AgentSessionRoleFilter::All);
        let page = page("codex-limit", &saved, 0, 50, &[], &[]);
        assert_eq!(page.items.len(), usize::from(text == "native-index-title"));
        assert_eq!(page.total, None);
        assert!(page.next_cursor.is_none());
        assert_eq!(
            page.source_states[0].state,
            AgentSessionSourceStatus::Partial
        );
        assert!(page.source_states[0]
            .message
            .as_deref()
            .unwrap()
            .contains("单文件超过读取上限"));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_missing_rollouts_keep_indexed_metadata_without_reusing_unavailable_bodies() {
    const REASON: &str = "原生会话日志不可用，仅能查看索引摘要";
    let root = std::env::temp_dir().join(format!(
        "balancehub-session-codex-missing-rollout-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let config_root = root.join("codex");
    let workdir = root.join("project");
    fs::create_dir_all(config_root.join("sessions")).unwrap();
    fs::create_dir_all(&workdir).unwrap();
    let rollout = config_root.join("sessions/main.jsonl");
    fs::write(
        &rollout,
        [
            json!({"type":"session_meta","payload":{"source":"cli"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"cached-body-marker"}}),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n"),
    )
    .unwrap();
    let outside = root.join("outside.jsonl");
    fs::write(
        &outside,
        json!({"type":"event_msg","payload":{"type":"user_message","message":"outside-body-marker"}}).to_string(),
    )
    .unwrap();
    let connection = rusqlite::Connection::open(config_root.join("state_5.sqlite")).unwrap();
    connection.execute_batch("CREATE TABLE threads (id TEXT, cwd TEXT, title TEXT, model TEXT, updated_at_ms INTEGER, rollout_path TEXT, source TEXT, parent_thread_id TEXT)").unwrap();
    for (id, title, source, parent, locator) in [
        (
            "main-indexed",
            Some("保留标题"),
            Some("cli"),
            None,
            "sessions/main.jsonl".into(),
        ),
        (
            "child-indexed",
            Some("子会话索引标题"),
            None,
            Some("main-indexed"),
            "sessions/child.jsonl".into(),
        ),
        (
            "untitled-indexed",
            None,
            None,
            None,
            "sessions/untitled.jsonl".into(),
        ),
        (
            "outside-indexed",
            Some("越界日志摘要"),
            Some("cli"),
            None,
            outside.to_string_lossy().to_string(),
        ),
    ] {
        connection.execute(
            "INSERT INTO threads VALUES (?1, ?2, ?3, 'fixture-model', 1786000005000, ?4, ?5, ?6)",
            rusqlite::params![id, workdir.to_string_lossy().as_ref(), title, locator, source, parent],
        ).unwrap();
    }
    drop(connection);
    let source = AgentSessionSource {
        id: hash(&[&config_root.to_string_lossy()]),
        agent_kind: AgentCliKind::Codex,
        config_root: config_root.to_string_lossy().to_string(),
        available: true,
    };
    let native = SessionHistorySource {
        config_root,
        launch_environment: EnvironmentPatch::default(),
        resume_reason: None,
    };
    let workspace = AgentSessionWorkspace {
        id: hash(&[&workdir.to_string_lossy()]),
        path: workdir.to_string_lossy().to_string(),
        exists: true,
        is_home: false,
    };
    let run = |query: &str, enabled: bool| {
        let limit = budget(4 * 1024 * 1024);
        let mut derived = index_config(&root);
        derived.enabled = enabled;
        with_read_budget(&limit, || {
            super::super::budget::with_source_root(&native.config_root, || {
                scan_source(
                    &source,
                    &native,
                    std::slice::from_ref(&workspace),
                    query,
                    &derived,
                    &limit,
                    Arc::new(Mutex::new(HashMap::new())),
                )
            })
        })
    };
    let seeded = run("cached-body-marker", true);
    let main_ref = seeded
        .references
        .iter()
        .find(|entry| entry.row.session.id == "main-indexed")
        .unwrap()
        .row
        .session_ref
        .clone();
    assert_eq!(seeded.matched.len(), 1);
    assert!(seeded.matched.contains(&main_ref));
    assert!(fs::read_dir(root.join("derived-index"))
        .unwrap()
        .next()
        .is_some());
    fs::remove_file(rollout).unwrap();

    for enabled in [false, true] {
        let listed = run("", enabled);
        assert_eq!(listed.references.len(), 4);
        assert_eq!(listed.matched.len(), 4);
        assert_eq!(listed.states[0].state, AgentSessionSourceStatus::Partial);
        let adapter = history_adapter(AgentCliKind::Codex).unwrap();
        for entry in &listed.references {
            assert!(!entry.row.session.can_resume);
            assert_eq!(entry.row.resume_reason.as_deref(), Some(REASON));
            assert_eq!(entry.row.source_id, source.id);
            assert_eq!(entry.row.workspace_id, workspace.id);
            assert_eq!(entry.row.session.workdir, workspace.path);
            assert_eq!(
                (adapter.detail)(
                    &entry.record,
                    crate::services::cli_sessions::DETAIL_READ_LIMITS
                )
                .unwrap_err(),
                REASON
            );
            match entry.row.session.id.as_str() {
                "main-indexed" => {
                    assert_eq!(entry.row.role, AgentSessionRole::Main);
                    assert_eq!(entry.row.session_ref, main_ref);
                }
                "child-indexed" => {
                    assert_eq!(entry.row.role, AgentSessionRole::Subagent);
                    assert!(matches!(
                        &entry.row.parent,
                        AgentSessionParent::Known { native_id, .. } if native_id == "main-indexed"
                    ));
                }
                "untitled-indexed" => assert_eq!(entry.row.role, AgentSessionRole::Unknown),
                "outside-indexed" => assert_eq!(entry.row.role, AgentSessionRole::Main),
                _ => panic!("unexpected native fixture"),
            }
        }
        let mut missing_workdir = workspace.clone();
        missing_workdir.exists = false;
        assert_eq!(
            row_for_record(
                &source,
                &native,
                &missing_workdir,
                &listed.references[0].record
            )
            .resume_reason
            .as_deref(),
            Some(REASON)
        );
        let complete = listed
            .states
            .iter()
            .all(|state| state.state == AgentSessionSourceStatus::Complete);
        let mut saved = snapshot(
            listed
                .references
                .into_iter()
                .map(|entry| entry.row)
                .collect(),
            complete,
        );
        saved.states = listed.states;
        let page = page("metadata-only", &saved, 0, 50, &[], &[]);
        assert_eq!(page.items.len(), 4);
        assert_eq!(page.total, None);
        for (query, expected) in [
            ("保留标题", 1),
            ("main-indexed", 1),
            ("untitled-indexed", 1),
            ("cached-body-marker", 0),
            ("outside-body-marker", 0),
        ] {
            let found = run(query, enabled);
            assert_eq!(
                found.matched.len(),
                expected,
                "query={query}, index={enabled}"
            );
            assert_eq!(found.states[0].state, AgentSessionSourceStatus::Partial);
        }
    }
    fs::remove_dir_all(root).unwrap();
}
