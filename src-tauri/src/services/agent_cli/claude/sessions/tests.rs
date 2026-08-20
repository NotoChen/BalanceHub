    use super::{
        encode_project_path, index_transcript, message_text, parse_transcript,
        parse_transcript_messages,
        search_transcript,
    };
    use crate::{
        models::{AgentCliKind, CliSessionMessageRole},
        services::agent_cli::contracts::{
            SessionContentSearchRequest, SessionIndexLoadResult, SessionReadLimits,
            SessionSearchTerm,
        },
    };
    use serde_json::json;
    use std::{fs, path::Path};

    const TEST_LIMITS: SessionReadLimits = SessionReadLimits {
        max_file_bytes: 1024 * 1024,
        max_messages: 100,
        max_total_chars: 100_000,
        max_message_chars: 10_000,
    };

    #[test]
    fn project_path_encoding_matches_claude_layout() {
        assert_eq!(
            encode_project_path(Path::new("/Users/example/project")),
            "-Users-example-project"
        );
    }

    #[test]
    fn message_text_handles_string_and_content_blocks() {
        assert_eq!(
            message_text(&json!({"message": {"content": "hello"}})).as_deref(),
            Some("hello")
        );
        assert_eq!(
            message_text(&json!({
                "message": {"content": [{"type": "text", "text": "hello"}, {"text": "world"}]}
            }))
            .as_deref(),
            Some("hello world")
        );
    }

    #[test]
    fn message_text_ignores_tool_results() {
        assert_eq!(
            message_text(&json!({
                "message": {
                    "content": [{"type": "tool_result", "content": "command output"}]
                }
            })),
            None
        );
        assert_eq!(
            message_text(&json!({
                "message": {
                    "content": [
                        {"type": "tool_result", "content": "command output"},
                        {"type": "text", "text": "actual request"}
                    ]
                }
            }))
            .as_deref(),
            Some("actual request")
        );
    }

    #[test]
    fn transcript_uses_the_first_real_user_message() {
        let root = std::env::temp_dir().join(format!(
            "balancehub-cli-session-claude-user-message-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let transcript = root.join("session.jsonl");
        let lines = [
            json!({
                "type": "user",
                "sessionId": "session-1",
                "cwd": root.to_string_lossy(),
                "message": {"content": [{"type": "tool_result", "content": "command output"}]}
            }),
            json!({
                "type": "user",
                "isMeta": true,
                "message": {"content": "internal context"}
            }),
            json!({
                "type": "user",
                "message": {"content": "actual request"}
            }),
        ]
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&transcript, lines).unwrap();
        let summary = parse_transcript(AgentCliKind::ClaudeCode, &transcript, &root)
            .unwrap()
            .unwrap();
        assert_eq!(summary.preview.as_deref(), Some("actual request"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn transcript_without_cwd_uses_the_selected_project_path() {
        let root = std::env::temp_dir().join(format!(
            "balancehub-cli-session-claude-workdir-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let transcript = root.join("session.jsonl");
        fs::write(
            &transcript,
            json!({
                "type": "user",
                "sessionId": "session-1",
                "message": {"content": "actual request"}
            })
            .to_string(),
        )
        .unwrap();
        let summary = parse_transcript(AgentCliKind::ClaudeCode, &transcript, &root)
            .unwrap()
            .unwrap();
        assert_eq!(summary.workdir, root.to_string_lossy());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn transcript_prefers_latest_ai_title_and_collects_models() {
        let root = std::env::temp_dir().join(format!(
            "balancehub-cli-session-claude-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let transcript = root.join("session.jsonl");
        let lines = [
            json!({
                "type": "user",
                "sessionId": "session-1",
                "cwd": root.to_string_lossy(),
                "timestamp": "2026-08-06T08:00:00Z",
                "message": {"content": "first request"}
            }),
            json!({
                "type": "assistant",
                "timestamp": "2026-08-06T08:01:00Z",
                "message": {"model": "claude-sonnet-4-5"}
            }),
            json!({
                "type": "ai-title",
                "aiTitle": "修复历史会话",
                "timestamp": "2026-08-06T08:02:00Z"
            }),
            json!({
                "type": "assistant",
                "timestamp": "2026-08-06T08:03:00Z",
                "message": {"model": "claude-opus-4-1"}
            }),
        ]
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&transcript, lines).unwrap();
        let summary = parse_transcript(AgentCliKind::ClaudeCode, &transcript, &root)
            .unwrap()
            .unwrap();
        assert_eq!(summary.id, "session-1");
        assert_eq!(summary.title, "修复历史会话");
        assert_eq!(summary.preview.as_deref(), Some("first request"));
        assert_eq!(summary.model.as_deref(), Some("claude-opus-4-1"));
        assert_eq!(summary.models.len(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn transcript_detail_keeps_messages_tools_and_filters_internal_records() {
        let root = std::env::temp_dir().join(format!(
            "balancehub-cli-session-claude-detail-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let transcript = root.join("session.jsonl");
        let lines = [
            json!({
                "type": "user",
                "sessionId": "session-1",
                "timestamp": "2026-08-19T08:00:00Z",
                "message": {"content": "<command-name>/clear</command-name>"}
            }),
            json!({
                "type": "user",
                "isMeta": true,
                "message": {"content": "内部上下文"}
            }),
            json!({
                "type": "user",
                "timestamp": "2026-08-19T08:01:00Z",
                "message": {"content": "真实问题"}
            }),
            json!({
                "type": "assistant",
                "timestamp": "2026-08-19T08:02:00Z",
                "message": {
                    "model": "claude-sonnet-4-5",
                    "content": [
                        {"type": "text", "text": "正在处理"},
                        {"type": "tool_use", "id": "tool-1", "name": "Read", "input": {"file_path": "src/App.vue"}}
                    ]
                }
            }),
            json!({
                "type": "user",
                "timestamp": "2026-08-19T08:03:00Z",
                "message": {"content": [{"type": "tool_result", "tool_use_id": "tool-1", "content": "文件内容"}]}
            }),
            json!({
                "type": "assistant",
                "isSidechain": true,
                "message": {"model": "claude-opus-4-1", "content": "子代理消息"}
            }),
        ]
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&transcript, lines).unwrap();

        let (messages, truncated, omitted) =
            parse_transcript_messages(&transcript, TEST_LIMITS).unwrap();
        assert!(!truncated);
        assert_eq!(omitted, 0);
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].role, CliSessionMessageRole::User);
        assert_eq!(messages[0].content, "真实问题");
        assert_eq!(messages[1].role, CliSessionMessageRole::Assistant);
        assert_eq!(messages[1].content, "正在处理");
        assert_eq!(messages[1].model.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(messages[2].role, CliSessionMessageRole::Tool);
        assert_eq!(messages[2].tool_name.as_deref(), Some("Read"));
        assert!(messages[2].content.contains("src/App.vue"));
        assert_eq!(messages[3].role, CliSessionMessageRole::Tool);
        assert_eq!(messages[3].tool_name.as_deref(), Some("Read"));
        assert_eq!(messages[3].content, "文件内容");
        assert!(!messages.iter().any(|message| message.content.contains("内部上下文")));
        assert!(!messages.iter().any(|message| message.content.contains("子代理消息")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn transcript_search_excludes_tool_results() {
        let root = std::env::temp_dir().join(format!(
            "balancehub-cli-session-claude-tool-search-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let transcript = root.join("session.jsonl");
        let output = format!("{}claude-tool-tail-keyword", "x".repeat(40 * 1024));
        fs::write(
            &transcript,
            json!({
                "type": "user",
                "message": {
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "tool-1",
                        "content": output
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();
        let request = SessionContentSearchRequest {
            terms: vec![SessionSearchTerm {
                index: 0,
                value: "claude-tool-tail-keyword".to_string(),
            }],
        };

        let result = search_transcript(&transcript, &request, &|| true).unwrap();

        assert!(result.matched_term_indexes.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn transcript_index_excludes_tools_meta_and_thinking_blocks() {
        let root = std::env::temp_dir().join(format!(
            "balancehub-cli-session-claude-index-filter-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let transcript = root.join("session.jsonl");
        let lines = [
            json!({
                "type": "user",
                "message": {"content": [{"type": "tool_result", "content": "hidden-tool-keyword"}]}
            }),
            json!({
                "type": "assistant",
                "isMeta": true,
                "message": {"content": "hidden-meta-keyword"}
            }),
            json!({
                "type": "assistant",
                "message": {"content": [
                    {"type": "thinking", "thinking": "hidden-thinking-keyword"},
                    {"type": "text", "text": "visible answer"}
                ]}
            }),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&transcript, lines).unwrap();

        let SessionIndexLoadResult::Updated { messages, .. } =
            index_transcript(&transcript, None, &|| true).unwrap()
        else {
            panic!("new source must be indexed");
        };
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "visible answer");
        fs::remove_dir_all(root).unwrap();
    }
