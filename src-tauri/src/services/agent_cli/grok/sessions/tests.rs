    use super::*;
    use crate::services::agent_cli::contracts::SessionSearchTerm;
    use serde_json::json;

    const TEST_LIMITS: SessionReadLimits = SessionReadLimits {
        max_file_bytes: 1024 * 1024,
        max_messages: 100,
        max_total_chars: 100_000,
        max_message_chars: 10_000,
    };

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "balancehub-grok-session-{name}-{}",
            std::process::id()
        ))
    }

    fn write_summary(root: &Path, name: &str, value: Value) {
        let directory = root.join("grok/sessions/project").join(name);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("summary.json"),
            serde_json::to_string_pretty(&value).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn official_summary_fields_drive_title_model_time_and_resume_id() {
        let root = test_root("metadata");
        let _ = fs::remove_dir_all(&root);
        let workdir = root.join("workspace");
        fs::create_dir_all(&workdir).unwrap();
        write_summary(
            &root,
            "session",
            json!({
                "info": {"id": "019c-grok-session", "cwd": workdir},
                "generated_title": "BalanceHub Grok 接入",
                "session_summary": "分析并接入 Grok Build",
                "created_at": "2026-08-14T08:00:00Z",
                "updated_at": "2026-08-14T08:01:00Z",
                "last_active_at": "2026-08-14T08:02:00Z",
                "num_messages": 4,
                "num_chat_messages": 2,
                "current_model_id": "grok-code-fast-1"
            }),
        );

        let sessions = list_from_home(AgentCliKind::Grok, &root.join("grok"), &workdir).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "019c-grok-session");
        assert_eq!(sessions[0].title, "BalanceHub Grok 接入");
        assert_eq!(sessions[0].preview.as_deref(), Some("分析并接入 Grok Build"));
        assert_eq!(sessions[0].model.as_deref(), Some("grok-code-fast-1"));
        assert_eq!(sessions[0].updated_at.as_deref(), Some("2026-08-14T08:02:00+00:00"));
        assert_eq!(sessions[0].metadata_source, "grokSummary");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn update_search_merges_streamed_chunks_and_replaces_tool_states() {
        let root = test_root("search-updates");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("updates.jsonl");
        let lines = [
            json!({"params":{"update":{"sessionUpdate":"agent_message_chunk","content":"Balance"}}}),
            json!({"params":{"update":{"sessionUpdate":"agent_message_chunk","content":"Hub"}}}),
            json!({"params":{"update":{"sessionUpdate":"tool_call_pending","toolCallId":"tool-1","title":"obsolete-keyword"}}}),
            json!({"params":{"update":{"sessionUpdate":"tool_call_completed","toolCallId":"tool-1","title":"完成"}}}),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&path, lines).unwrap();
        let request = |value: &str| SessionContentSearchRequest {
            terms: vec![SessionSearchTerm {
                index: 0,
                value: value.to_string(),
            }],
        };

        let streamed = search_updates(&path, &request("balancehub"), &|| true).unwrap();
        assert_eq!(streamed.matched_term_indexes, vec![0]);
        let replaced = search_updates(&path, &request("obsolete-keyword"), &|| true).unwrap();
        assert!(replaced.matched_term_indexes.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn streamed_search_tracks_character_count_without_rescanning_accumulated_text() {
        let request = SessionContentSearchRequest {
            terms: vec![SessionSearchTerm {
                index: 0,
                value: "你好balancehub".to_string(),
            }],
        };
        let mut state = GrokUpdateSearchState::new(&request);
        state.observe(
            0,
            &json!({"sessionUpdate": "agent_message_chunk", "content": "你好"}),
        );
        state.observe(
            1,
            &json!({"sessionUpdate": "agent_message_chunk", "content": "BalanceHub"}),
        );

        let current = state.current.as_ref().unwrap();
        assert_eq!(current.char_count, "你好BalanceHub".chars().count());
        let result = state.finish();
        assert_eq!(result.matched_term_indexes, vec![0]);
    }

    #[test]
    fn streamed_search_splits_a_single_oversized_chunk_into_bounded_messages() {
        let request = SessionContentSearchRequest {
            terms: vec![SessionSearchTerm {
                index: 0,
                value: "grok-oversized-tail".to_string(),
            }],
        };
        let mut state = GrokUpdateSearchState::new(&request);
        let content = format!(
            "{}grok-oversized-tail",
            "x".repeat(MAX_STREAMED_MESSAGE_CHARS + 32)
        );
        state.observe(
            0,
            &json!({"sessionUpdate": "agent_message_chunk", "content": content}),
        );

        let messages = state.messages.len();
        let result = state.finish();

        assert!(messages >= 1);
        assert_eq!(result.matched_term_indexes, vec![0]);
    }

    #[test]
    fn search_excludes_tool_updates_and_history_parts() {
        let root = test_root("long-tool-search");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let request = |value: &str| SessionContentSearchRequest {
            terms: vec![SessionSearchTerm {
                index: 0,
                value: value.to_string(),
            }],
        };

        let updates_path = root.join("updates.jsonl");
        let update_output = format!("{}grok-update-tail-keyword", "x".repeat(40 * 1024));
        fs::write(
            &updates_path,
            json!({
                "params": {
                    "update": {
                        "sessionUpdate": "tool_call_completed",
                        "toolCallId": "tool-1",
                        "rawOutput": update_output
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        let update_result = search_updates(
            &updates_path,
            &request("grok-update-tail-keyword"),
            &|| true,
        )
        .unwrap();
        assert!(update_result.matched_term_indexes.is_empty());

        let history_path = root.join("chat_history.jsonl");
        let history_output = format!("{}grok-history-tail-keyword", "x".repeat(40 * 1024));
        fs::write(
            &history_path,
            json!({
                "type": "assistant",
                "content": [{
                    "type": "tool_call",
                    "name": "Search",
                    "result": history_output
                }]
            })
            .to_string(),
        )
        .unwrap();
        let history_result = search_chat_history(
            &history_path,
            &request("grok-history-tail-keyword"),
            &|| true,
        )
        .unwrap();
        assert!(history_result.matched_term_indexes.is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn chat_history_index_excludes_tools_and_hidden_reasoning() {
        let root = test_root("index-filter");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("chat_history.jsonl");
        let lines = [
            json!({"type": "thought", "data": "hidden-thought-keyword"}),
            json!({
                "type": "assistant",
                "content": [
                    {"type": "reasoning", "text": "hidden-reasoning-keyword"},
                    {"type": "text", "text": "visible answer"},
                    {"type": "tool_call", "name": "Search", "result": "hidden-tool-keyword"}
                ]
            }),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&path, lines).unwrap();

        let messages = index_chat_history(&path, &|| true).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "visible answer");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hidden_subagent_empty_and_other_workspace_sessions_are_filtered() {
        let root = test_root("filtered");
        let _ = fs::remove_dir_all(&root);
        let workdir = root.join("workspace");
        fs::create_dir_all(&workdir).unwrap();
        write_summary(
            &root,
            "hidden",
            json!({
                "info": {"id": "hidden", "cwd": workdir},
                "hidden": true,
                "session_summary": "hidden",
                "num_messages": 1
            }),
        );
        write_summary(
            &root,
            "subagent",
            json!({
                "info": {"id": "subagent", "cwd": workdir},
                "session_kind": "subagent_resume",
                "session_summary": "subagent",
                "num_messages": 1
            }),
        );
        write_summary(
            &root,
            "empty",
            json!({
                "info": {"id": "empty", "cwd": workdir},
                "session_summary": "empty",
                "num_messages": 0,
                "num_chat_messages": 0
            }),
        );
        write_summary(
            &root,
            "other",
            json!({
                "info": {"id": "other", "cwd": root.join("other")},
                "session_summary": "other",
                "num_messages": 1
            }),
        );

        assert!(list_from_home(AgentCliKind::Grok, &root.join("grok"), &workdir)
            .unwrap()
            .is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_session_ids_keep_the_most_recent_summary() {
        let root = test_root("duplicate");
        let _ = fs::remove_dir_all(&root);
        let workdir = root.join("workspace");
        fs::create_dir_all(&workdir).unwrap();
        for (name, title, updated_at) in [
            ("older", "旧标题", "2026-08-14T08:00:00Z"),
            ("newer", "新标题", "2026-08-14T09:00:00Z"),
        ] {
            write_summary(
                &root,
                name,
                json!({
                    "info": {"id": "same-session", "cwd": workdir},
                    "generated_title": title,
                    "updated_at": updated_at,
                    "num_messages": 1,
                    "current_model_id": "grok-code-fast-1"
                }),
            );
        }

        let sessions = list_from_home(AgentCliKind::Grok, &root.join("grok"), &workdir).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "新标题");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn updates_detail_keeps_user_agent_and_tool_events() {
        let root = test_root("updates-detail");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("updates.jsonl");
        let lines = [
            json!({
                "timestamp": 1_776_585_600,
                "params": {"update": {"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "真实问题"}}}
            }),
            json!({
                "timestamp": 1_776_585_660,
                "params": {"update": {"sessionUpdate": "agent_message_chunk", "content": [{"type": "text", "text": "处理"}], "_meta": {"modelId": "grok-code-fast-1"}}}
            }),
            json!({
                "timestamp": 1_776_585_661,
                "params": {"update": {"sessionUpdate": "agent_message_chunk", "content": [{"type": "text", "text": "完成"}], "_meta": {"modelId": "grok-code-fast-1"}}}
            }),
            json!({
                "timestamp": 1_776_585_720,
                "params": {"update": {"sessionUpdate": "tool_call_update", "toolCallId": "tool-1", "title": "Read", "status": "pending", "_meta": {"modelId": "grok-code-fast-1"}}}
            }),
            json!({
                "timestamp": 1_776_585_721,
                "params": {"update": {"sessionUpdate": "tool_call_update", "toolCallId": "tool-1", "title": "Read", "status": "completed", "rawOutput": "文件内容", "_meta": {"modelId": "grok-code-fast-1"}}}
            }),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&path, lines).unwrap();

        let (messages, truncated, omitted) = parse_updates(&path, TEST_LIMITS).unwrap();
        assert!(!truncated);
        assert_eq!(omitted, 0);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, CliSessionMessageRole::User);
        assert_eq!(messages[0].content, "真实问题");
        assert_eq!(messages[1].role, CliSessionMessageRole::Assistant);
        assert_eq!(messages[1].content, "处理完成");
        assert_eq!(messages[1].model.as_deref(), Some("grok-code-fast-1"));
        assert_eq!(messages[2].role, CliSessionMessageRole::Tool);
        assert_eq!(messages[2].tool_name.as_deref(), Some("Read"));
        assert!(messages[2].content.contains("文件内容"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn chat_history_detail_is_a_readable_fallback() {
        let root = test_root("chat-history-detail");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("chat_history.jsonl");
        let lines = [
            json!({"type": "user", "timestamp": "2026-08-19T08:00:00Z", "content": "用户问题"}),
            json!({
                "type": "assistant",
                "timestamp": "2026-08-19T08:01:00Z",
                "model": "grok-code-fast-1",
                "content": [
                    {"type": "text", "text": "Agent 回复"},
                    {"type": "tool_call", "name": "Search", "arguments": {"query": "BalanceHub"}}
                ]
            }),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&path, lines).unwrap();

        let (messages, truncated, omitted) = parse_chat_history(&path, TEST_LIMITS).unwrap();
        assert!(!truncated);
        assert_eq!(omitted, 0);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content, "用户问题");
        assert_eq!(messages[1].content, "Agent 回复");
        assert_eq!(messages[2].role, CliSessionMessageRole::Tool);
        assert_eq!(messages[2].tool_name.as_deref(), Some("Search"));
        assert!(messages[2].content.contains("BalanceHub"));
        fs::remove_dir_all(root).unwrap();
    }
