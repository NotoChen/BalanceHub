use super::*;
use crate::{
    models::{CliSessionMessageRole, CliSessionSummary},
    services::agent_cli::contracts::SessionIndexMessage,
};

fn test_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "balancehub-session-index-{name}-{}",
        std::process::id()
    ))
}

fn summary(id: &str, workdir: &Path) -> CliSessionSummary {
    CliSessionSummary {
        id: id.to_string(),
        title: "会话索引测试".to_string(),
        preview: Some("只保存可见正文".to_string()),
        model: Some("model-test".to_string()),
        models: vec!["model-test".to_string()],
        cli_kind: AgentCliKind::Codex,
        created_at: None,
        updated_at: Some("2026-08-19T08:00:00Z".to_string()),
        workdir: workdir.to_string_lossy().to_string(),
        cli_version: None,
        archived: false,
        can_resume: true,
        metadata_source: "test".to_string(),
    }
}

fn read_budget(max_bytes: u64) -> SessionReadBudget {
    SessionReadBudget::new(
        Instant::now() + Duration::from_secs(30),
        Arc::new(AtomicBool::new(false)),
        max_bytes,
    )
}

#[test]
fn trigram_index_finds_chinese_substrings_and_short_terms() {
    let root = test_root("search");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let database = root.join("codex.sqlite3");
    let mut connection = open_connection(&database, true).unwrap();
    let workdir = root.join("workspace");
    fs::create_dir_all(&workdir).unwrap();
    let session = summary("session-1", &workdir);
    replace_session(
        &mut connection,
        &workspace_key(&workdir),
        &session,
        "fingerprint",
        128,
        vec![SessionIndexMessage {
            id: "message-1".to_string(),
            role: CliSessionMessageRole::Assistant,
            content: "BalanceHub 会话全文检索已经完成".to_string(),
        }],
    )
    .unwrap();

    for term in ["全文检索", "会话"] {
        let request = crate::services::agent_cli::contracts::SessionContentSearchRequest {
            terms: vec![crate::services::agent_cli::contracts::SessionSearchTerm {
                index: 0,
                value: term.to_string(),
            }],
        };
        let result =
            search_indexed_messages(&connection, &workspace_key(&workdir), &session.id, &request)
                .unwrap();
        assert_eq!(result.matched_term_indexes, vec![0]);
    }
    connection
        .close()
        .map_err(|(_, error)| error)
        .expect("the SQLite test connection should close before cleanup");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn message_chunking_preserves_cross_boundary_search_text() {
    let content = format!(
        "{}跨块关键字{}",
        "x".repeat(MESSAGE_CHUNK_CHARS - 4),
        "y".repeat(20)
    );
    let chunks = chunk_message(&content);
    assert!(chunks.len() >= 2);
    assert!(chunks.iter().any(|chunk| chunk.contains("跨块关键字")));
}

#[test]
fn message_chunking_uses_utf8_boundaries() {
    let content = format!("{}尾部", "会".repeat(MESSAGE_CHUNK_CHARS + 8));
    let chunks = chunk_message(&content);
    assert_eq!(chunks[0].chars().count(), MESSAGE_CHUNK_CHARS);
    assert!(chunks[1].starts_with('会'));
    assert!(chunks[1].ends_with("尾部"));
}

#[test]
fn capacity_is_shared_across_agent_databases() {
    let root = test_root("capacity");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let config = SessionIndexConfig {
        enabled: true,
        directory: root.clone(),
        max_size_bytes: 1,
    };
    for cli_kind in [AgentCliKind::Codex, AgentCliKind::ClaudeCode] {
        let database = database_path(&config, cli_kind);
        let mut connection = open_connection(&database, true).unwrap();
        let workdir = root.join(cli_kind.key());
        fs::create_dir_all(&workdir).unwrap();
        let mut session = summary(cli_kind.key(), &workdir);
        session.cli_kind = cli_kind;
        replace_session(
            &mut connection,
            &workspace_key(&workdir),
            &session,
            "fingerprint",
            64,
            vec![SessionIndexMessage {
                id: "message".to_string(),
                role: CliSessionMessageRole::User,
                content: "需要被容量约束管理的正文".repeat(200),
            }],
        )
        .unwrap();
    }
    HistoryIndex::new(&config, AgentCliKind::Codex, "test-source")
        .finish(&read_budget(u64::MAX))
        .unwrap();
    let remaining = AgentCliKind::ALL
        .iter()
        .filter_map(|&kind| {
            let path = database_path(&config, kind);
            path.is_file().then(|| {
                open_connection(&path, false)
                    .unwrap()
                    .query_row("SELECT COUNT(*) FROM sessions", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .unwrap_or_default()
            })
        })
        .sum::<i64>();
    assert_eq!(remaining, 0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn shared_capacity_keeps_valid_indexes_when_partial_eviction_is_enough() {
    let root = test_root("capacity-partial");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let mut config = SessionIndexConfig {
        enabled: true,
        directory: root.clone(),
        max_size_bytes: u64::MAX,
    };

    for &cli_kind in AgentCliKind::ALL {
        let database = database_path(&config, cli_kind);
        let mut connection = open_connection(&database, true).unwrap();
        let workdir = root.join(cli_kind.key());
        fs::create_dir_all(&workdir).unwrap();
        let mut session = summary(cli_kind.key(), &workdir);
        session.cli_kind = cli_kind;
        let content = (0..2_000)
            .map(|index| format!("{}-{index:04x}", cli_kind.key()))
            .collect::<Vec<_>>()
            .join(" ");
        replace_session(
            &mut connection,
            &workspace_key(&workdir),
            &session,
            "fingerprint",
            content.len() as u64,
            vec![SessionIndexMessage {
                id: "message".to_string(),
                role: CliSessionMessageRole::Assistant,
                content,
            }],
        )
        .unwrap();
    }

    let total_before = total_disk_size(&config);
    let largest_database = AgentCliKind::ALL
        .iter()
        .map(|&kind| database_disk_size(&database_path(&config, kind)))
        .max()
        .unwrap();
    config.max_size_bytes = total_before.saturating_sub(largest_database / 3);

    HistoryIndex::new(&config, AgentCliKind::Codex, "test-source")
        .finish(&read_budget(u64::MAX))
        .unwrap();

    let remaining = AgentCliKind::ALL
        .iter()
        .filter_map(|&kind| {
            let path = database_path(&config, kind);
            path.is_file().then(|| {
                open_connection(&path, false)
                    .unwrap()
                    .query_row("SELECT COUNT(*) FROM sessions", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .unwrap_or_default()
            })
        })
        .sum::<i64>();
    assert!(remaining > 0, "partial pressure must not erase every index");
    assert!(remaining < AgentCliKind::ALL.len() as i64);
    assert!(total_disk_size(&config) <= config.max_size_bytes);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn capacity_maintenance_obeys_row_budget_before_deleting_existing_indexes() {
    let root = test_root("bounded-capacity");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let config = SessionIndexConfig {
        enabled: true,
        directory: root.clone(),
        max_size_bytes: 1,
    };
    let database = database_path(&config, AgentCliKind::Codex);
    let mut connection = open_connection(&database, true).unwrap();
    for index in 0..8 {
        replace_session(
            &mut connection,
            "native-workspace",
            &summary(&format!("session-{index}"), &root),
            "fingerprint",
            128,
            vec![SessionIndexMessage {
                id: "message".into(),
                role: CliSessionMessageRole::User,
                content: "derived content".repeat(100),
            }],
        )
        .unwrap();
    }
    drop(connection);
    let index = HistoryIndex::new(&config, AgentCliKind::Codex, "test-source");
    let budget = read_budget(1);
    let started = Instant::now();
    assert!(index.finish(&budget).unwrap_err().contains("字节预算"));
    assert!(started.elapsed() < Duration::from_secs(2));
    let connection = open_connection(&database, false).unwrap();
    let remaining: i64 = connection
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        remaining, 8,
        "an interrupted enumeration must not start eviction"
    );
    drop(connection);
    // Cancellation released every gate; a later independent pass can finish.
    index.finish(&read_budget(u64::MAX)).unwrap();
    assert!(database_files(&config).is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn sqlite_progress_interrupts_long_statements_on_cancel_or_index_invalidation() {
    let root = test_root("sql-progress");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    for invalidate_settings in [false, true] {
        let budget = read_budget(u64::MAX);
        let invalidated = Arc::new(AtomicBool::new(false));
        let connection = open_connection_with_budget(
            &root.join("codex.sqlite3"),
            true,
            Some(IndexBudget {
                read: &budget,
                invalidated: &invalidated,
            }),
        )
        .unwrap();
        let cancelled = if invalidate_settings {
            invalidated
        } else {
            budget.cancelled.clone()
        };
        let started = Instant::now();
        let result = std::thread::scope(|threads| {
            threads.spawn(move || {
                std::thread::sleep(Duration::from_millis(10));
                cancelled.store(true, Ordering::Release);
            });
            connection.query_row(
                "WITH RECURSIVE work(value) AS (
                     SELECT 1 UNION ALL SELECT value + 1 FROM work WHERE value < 1000000000
                 ) SELECT SUM(value) FROM work",
                [],
                |row| row.get::<_, i64>(0),
            )
        });
        assert!(
            matches!(result, Err(rusqlite::Error::SqliteFailure(error, _))
            if error.code == rusqlite::ErrorCode::OperationInterrupted)
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn index_connection_lock_wait_stops_when_the_request_is_cancelled() {
    let root = test_root("cancel-lock-wait");
    let config = SessionIndexConfig {
        enabled: true,
        directory: root,
        max_size_bytes: u64::MAX,
    };
    let index = HistoryIndex::new(&config, AgentCliKind::Codex, "test-source");
    let gate = database_gate(&index.path);
    let _held = gate.lock().unwrap();
    let budget = read_budget(u64::MAX);
    let cancelled = budget.cancelled.clone();
    let started = Instant::now();
    let result = std::thread::scope(|threads| {
        threads.spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            cancelled.store(true, Ordering::Release);
        });
        index.connection(&budget, |_| -> Result<(), String> {
            panic!("cancelled work must not open the held database");
        })
    });
    assert!(result.unwrap_err().contains("已取消"));
    assert!(started.elapsed() < Duration::from_secs(2));
}

fn database_path(config: &SessionIndexConfig, kind: AgentCliKind) -> PathBuf {
    source_database_path(config, kind, "test-source")
}

fn workspace_key(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn total_disk_size(config: &SessionIndexConfig) -> u64 {
    total_disk_size_with_budget(config, None).unwrap_or_default()
}

fn replace_session(
    connection: &mut Connection,
    workspace: &str,
    session: &CliSessionSummary,
    fingerprint: &str,
    source_bytes: u64,
    messages: Vec<SessionIndexMessage>,
) -> Result<(), String> {
    let indexed_bytes = session
        .title
        .len()
        .saturating_add(session.preview.as_deref().map(str::len).unwrap_or_default())
        .saturating_add(session.model.as_deref().map(str::len).unwrap_or_default())
        .saturating_add(session.models.iter().map(String::len).sum::<usize>())
        .saturating_add(
            messages
                .iter()
                .map(|message| message.id.len().saturating_add(message.content.len()))
                .sum::<usize>(),
        );
    let transaction = connection
        .transaction()
        .map_err(|error| format!("开始会话索引事务失败: {error}"))?;
    transaction
        .execute(
            "DELETE FROM messages WHERE workspace=?1 AND session_id=?2",
            params![workspace, session.id],
        )
        .map_err(|error| format!("删除旧会话消息失败: {error}"))?;
    transaction
        .execute(
            "INSERT OR REPLACE INTO sessions(
                 workspace, session_id, title, preview, model, models_json, workdir,
                 created_at, updated_at, fingerprint, source_bytes, indexed_bytes, indexed_at,
                 last_used_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                workspace,
                session.id,
                session.title,
                session.preview,
                session.model,
                serde_json::to_string(&session.models).unwrap_or_else(|_| "[]".to_string()),
                session.workdir,
                session.created_at,
                session.updated_at,
                fingerprint,
                source_bytes as i64,
                indexed_bytes as i64,
                chrono::Utc::now().to_rfc3339(),
                unix_millis() as i64,
            ],
        )
        .map_err(|error| format!("写入会话索引摘要失败: {error}"))?;
    for message in messages {
        crate::services::cli_sessions::workbench::check_read_budget()?;
        if !matches!(
            message.role,
            CliSessionMessageRole::User | CliSessionMessageRole::Assistant
        ) {
            continue;
        }
        for (chunk_index, content) in chunk_message(&message.content).into_iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO messages(workspace, session_id, message_id, role, chunk_index, content)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        workspace,
                        session.id,
                        message.id,
                        role_key(message.role),
                        chunk_index as i64,
                        content,
                    ],
                )
                .map_err(|error| format!("写入会话消息索引失败: {error}"))?;
        }
    }
    transaction
        .commit()
        .map_err(|error| format!("提交会话索引失败: {error}"))
}

mod incremental;
