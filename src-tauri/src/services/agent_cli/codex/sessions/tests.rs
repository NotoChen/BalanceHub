    use super::{
        codex_home, index_rollout, parse_rollout_messages, read_database, read_session_titles,
        resolve_rollout_path, search_rollout,
    };
    use crate::{
        models::{AgentCliKind, CliSessionMessageRole},
        services::agent_cli::contracts::{
            SessionContentSearchRequest, SessionIndexLoadResult, SessionReadLimits,
            SessionSearchTerm,
        },
    };
    use rusqlite::Connection;
    use serde_json::json;
    use std::{fs, path::PathBuf};

    const TEST_LIMITS: SessionReadLimits = SessionReadLimits {
        max_file_bytes: 1024 * 1024,
        max_messages: 100,
        max_total_chars: 100_000,
        max_message_chars: 10_000,
    };

    #[test]
    fn default_home_uses_user_home() {
        assert!(codex_home().is_ok());
    }

    #[test]
    fn state_database_maps_title_preview_model_and_dates() {
        let path = std::env::temp_dir().join(format!(
            "balancehub-cli-session-codex-test-{}.sqlite",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE threads (id TEXT, cwd TEXT, name TEXT, title TEXT, preview TEXT, first_user_message TEXT, model TEXT, created_at_ms INTEGER, updated_at_ms INTEGER, archived INTEGER, cli_version TEXT); INSERT INTO threads VALUES ('session-1', '/tmp/project', '', '显式标题', '摘要', '首条请求', 'gpt-5.5', 1786000000000, 1786000005000, 0, '0.146.0');",
            )
            .unwrap();
        drop(connection);
        let sessions = read_database(
            AgentCliKind::Codex,
            &path,
            "/tmp/project",
            "/tmp/project",
            10,
        )
        .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "显式标题");
        assert_eq!(sessions[0].preview.as_deref(), Some("摘要"));
        assert_eq!(sessions[0].model.as_deref(), Some("gpt-5.5"));
        assert!(sessions[0].created_at.is_some());
        assert_eq!(sessions[0].cli_version.as_deref(), Some("0.146.0"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn official_session_name_overrides_database_title() {
        let directory = std::env::temp_dir().join(format!(
            "balancehub-cli-session-index-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("session_index.jsonl"),
            concat!(
                "{\"id\":\"session-1\",\"thread_name\":\"BalanceHub\"}\n",
                "not-json\n",
                "{\"id\":\"session-2\",\"thread_name\":\"  \"}\n",
                "{\"session_id\":\"session-3\",\"name\":\"命名会话\"}\n",
            ),
        )
        .unwrap();

        let titles = read_session_titles(&directory).unwrap();
        assert_eq!(
            titles.get("session-1").map(String::as_str),
            Some("BalanceHub")
        );
        assert_eq!(
            titles.get("session-3").map(String::as_str),
            Some("命名会话")
        );
        assert!(!titles.contains_key("session-2"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rollout_messages_prefer_current_events_and_keep_tool_calls() {
        let directory = std::env::temp_dir().join(format!(
            "balancehub-cli-session-codex-rollout-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("rollout.jsonl");
        let lines = [
            json!({"type":"turn_context","payload":{"model":"gpt-5.6"}}),
            json!({"type":"event_msg","timestamp":"2026-08-19T08:00:00Z","payload":{"type":"user_message","message":"当前用户消息"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"旧格式重复消息"}]}}),
            json!({"type":"response_item","payload":{"type":"function_call","call_id":"call-1","name":"read_file","arguments":"{\"path\":\"src/App.vue\"}"}}),
            json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"读取完成"}}),
            json!({"type":"event_msg","timestamp":"2026-08-19T08:01:00Z","payload":{"type":"agent_message","message":"当前 Agent 回复"}}),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&path, lines).unwrap();

        let (messages, truncated, omitted) = parse_rollout_messages(&path, TEST_LIMITS).unwrap();
        assert!(!truncated);
        assert_eq!(omitted, 0);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, CliSessionMessageRole::User);
        assert_eq!(messages[0].content, "当前用户消息");
        assert_eq!(messages[1].role, CliSessionMessageRole::Tool);
        assert_eq!(messages[1].tool_name.as_deref(), Some("read_file"));
        assert!(messages[1].content.contains("src/App.vue"));
        assert!(messages[1].content.contains("读取完成"));
        assert_eq!(messages[2].role, CliSessionMessageRole::Assistant);
        assert_eq!(messages[2].model.as_deref(), Some("gpt-5.6"));
        assert!(!messages.iter().any(|message| message.content == "旧格式重复消息"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rollout_messages_fall_back_to_legacy_response_messages() {
        let directory = std::env::temp_dir().join(format!(
            "balancehub-cli-session-codex-legacy-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("rollout.jsonl");
        fs::write(
            &path,
            [
                json!({"type":"turn_context","payload":{"model":"gpt-5.5"}}),
                json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"旧用户消息"}]}}),
                json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"旧 Agent 回复"}]}}),
            ]
            .into_iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
        )
        .unwrap();

        let (messages, _, _) = parse_rollout_messages(&path, TEST_LIMITS).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "旧用户消息");
        assert_eq!(messages[1].content, "旧 Agent 回复");
        assert_eq!(messages[1].model.as_deref(), Some("gpt-5.5"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rollout_path_fallback_finds_dated_session_directories() {
        let directory = std::env::temp_dir().join(format!(
            "balancehub-cli-session-codex-path-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        let nested = directory.join("sessions/2026/08/19");
        fs::create_dir_all(&nested).unwrap();
        let rollout = nested.join("rollout-session-1.jsonl");
        fs::write(&rollout, "{}\n").unwrap();

        let stale = PathBuf::from("/old/codex/sessions/rollout-session-1.jsonl");
        assert_eq!(
            resolve_rollout_path(&directory, &stale.to_string_lossy()),
            Some(rollout)
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rollout_search_scans_beyond_detail_windows() {
        let directory = std::env::temp_dir().join(format!(
            "balancehub-cli-session-codex-search-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("rollout.jsonl");
        let lines = (0..3_000)
            .map(|index| {
                json!({
                    "type": "event_msg",
                    "payload": {
                        "type": "user_message",
                        "message": if index == 1_777 { "middle-codex-keyword".to_string() } else { "x".repeat(2_048) }
                    }
                })
                .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, lines).unwrap();
        let request = SessionContentSearchRequest {
            terms: vec![SessionSearchTerm {
                index: 0,
                value: "middle-codex-keyword".to_string(),
            }],
        };
        let result = search_rollout(&path, &request, &|| true).unwrap();
        assert_eq!(result.matched_term_indexes, vec![0]);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rollout_search_ignores_legacy_messages_when_primary_events_exist() {
        let directory = std::env::temp_dir().join(format!(
            "balancehub-cli-session-codex-search-source-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("rollout.jsonl");
        fs::write(
            &path,
            [
                json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"legacy-only-keyword"}]}}),
                json!({"type":"event_msg","payload":{"type":"user_message","message":"当前消息"}}),
            ]
            .into_iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
        )
        .unwrap();
        let request = SessionContentSearchRequest {
            terms: vec![SessionSearchTerm {
                index: 0,
                value: "legacy-only-keyword".to_string(),
            }],
        };
        let result = search_rollout(&path, &request, &|| true).unwrap();
        assert!(result.matched_term_indexes.is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rollout_search_excludes_tool_outputs() {
        let directory = std::env::temp_dir().join(format!(
            "balancehub-cli-session-codex-tool-search-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("rollout.jsonl");
        let output = format!("{}codex-tool-tail-keyword", "x".repeat(40 * 1024));
        fs::write(
            &path,
            json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call_output",
                    "call_id": "call-1",
                    "output": output
                }
            })
            .to_string(),
        )
        .unwrap();
        let request = SessionContentSearchRequest {
            terms: vec![SessionSearchTerm {
                index: 0,
                value: "codex-tool-tail-keyword".to_string(),
            }],
        };

        let result = search_rollout(&path, &request, &|| true).unwrap();

        assert!(result.matched_term_indexes.is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rollout_index_keeps_visible_messages_only() {
        let directory = std::env::temp_dir().join(format!(
            "balancehub-cli-session-codex-index-filter-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("rollout.jsonl");
        let lines = [
            json!({
                "type": "response_item",
                "payload": {
                    "type": "reasoning",
                    "content": [{"type": "reasoning", "text": "hidden-reasoning-keyword"}]
                }
            }),
            json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call_output",
                    "call_id": "call-1",
                    "output": "hidden-tool-keyword"
                }
            }),
            json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {"type": "reasoning", "text": "hidden-part-keyword"},
                        {"type": "output_text", "text": "visible answer"}
                    ]
                }
            }),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&path, lines).unwrap();

        let SessionIndexLoadResult::Updated { messages, .. } =
            index_rollout(&path, None, &|| true).unwrap()
        else {
            panic!("new source must be indexed");
        };
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "visible answer");
        fs::remove_dir_all(directory).unwrap();
    }
