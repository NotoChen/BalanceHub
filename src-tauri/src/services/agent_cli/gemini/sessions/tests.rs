    use super::{
        index_conversation, list_from_config_dir, load_conversation_limited, search_conversation,
    };
    use crate::{
        models::AgentCliKind,
        services::agent_cli::contracts::{
            SessionContentSearchRequest, SessionIndexLoadResult, SessionReadLimits,
            SessionSearchTerm,
        },
    };
    use serde_json::json;
    use std::fs;

    const TEST_LIMITS: SessionReadLimits = SessionReadLimits {
        max_file_bytes: 1024 * 1024,
        max_messages: 100,
        max_total_chars: 100_000,
        max_message_chars: 10_000,
    };

    fn test_root(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "balancehub-gemini-session-{name}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn official_jsonl_metadata_drives_title_model_and_resume_id() {
        let root = test_root("metadata");
        let _ = fs::remove_dir_all(&root);
        let workdir = root.join("workspace");
        let chats = root.join("config/tmp/workspace/chats");
        fs::create_dir_all(&workdir).unwrap();
        fs::create_dir_all(&chats).unwrap();
        fs::write(
            root.join("config/projects.json"),
            json!({"projects": {workdir.to_string_lossy(): "workspace"}}).to_string(),
        )
        .unwrap();
        let lines = [
            json!({
                "sessionId": "019c-gemini-session",
                "projectHash": "hash",
                "startTime": "2026-08-14T08:00:00Z",
                "lastUpdated": "2026-08-14T08:01:00Z"
            }),
            json!({"id": "user-1", "type": "user", "content": [{"text": "接入 Gemini CLI"}]}),
            json!({"id": "gemini-1", "type": "gemini", "content": "处理中", "model": "gemini-2.5-pro"}),
            json!({"id": "gemini-1", "type": "gemini", "content": "完成", "model": "gemini-3-pro"}),
            json!({"$set": {"summary": "BalanceHub Gemini 接入", "lastUpdated": "2026-08-14T08:02:00Z"}}),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(chats.join("session.jsonl"), lines).unwrap();

        let sessions = list_from_config_dir(
            AgentCliKind::Gemini,
            &root.join("config"),
            &workdir,
        )
        .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "019c-gemini-session");
        assert_eq!(sessions[0].title, "BalanceHub Gemini 接入");
        assert_eq!(sessions[0].preview.as_deref(), Some("接入 Gemini CLI"));
        assert_eq!(sessions[0].model.as_deref(), Some("gemini-3-pro"));
        assert_eq!(sessions[0].models.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rewinds_and_ignored_commands_do_not_create_wrong_titles() {
        let root = test_root("rewind");
        let _ = fs::remove_dir_all(&root);
        let workdir = root.join("workspace");
        let project = root.join("config/tmp/workspace");
        let chats = project.join("chats");
        fs::create_dir_all(&workdir).unwrap();
        fs::create_dir_all(&chats).unwrap();
        fs::write(
            project.join(".project_root"),
            workdir.to_string_lossy().as_bytes(),
        )
        .unwrap();
        let lines = [
            json!({"sessionId": "rewind-session", "projectHash": "hash"}),
            json!({"id": "command", "type": "user", "content": "/help"}),
            json!({"id": "discarded", "type": "user", "content": "应该被回退"}),
            json!({"$rewindTo": "discarded"}),
            json!({"id": "kept", "type": "user", "content": "最终问题"}),
            json!({"id": "answer", "type": "gemini", "content": "完成", "model": "gemini-2.5-flash"}),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(chats.join("session.jsonl"), lines).unwrap();

        let sessions = list_from_config_dir(
            AgentCliKind::Gemini,
            &root.join("config"),
            &workdir,
        )
        .unwrap();
        assert_eq!(sessions[0].title, "最终问题");
        assert_eq!(sessions[0].preview.as_deref(), Some("最终问题"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn subagent_and_empty_sessions_are_filtered() {
        let root = test_root("filtered");
        let _ = fs::remove_dir_all(&root);
        let workdir = root.join("workspace");
        let chats = root.join("config/tmp/workspace/chats");
        fs::create_dir_all(&workdir).unwrap();
        fs::create_dir_all(&chats).unwrap();
        fs::write(
            root.join("config/projects.json"),
            json!({"projects": {workdir.to_string_lossy(): "workspace"}}).to_string(),
        )
        .unwrap();
        fs::write(
            chats.join("subagent.jsonl"),
            concat!(
                "{\"sessionId\":\"subagent\",\"projectHash\":\"hash\",\"kind\":\"subagent\"}\n",
                "{\"id\":\"user\",\"type\":\"user\",\"content\":\"hidden\"}\n"
            ),
        )
        .unwrap();
        fs::write(
            chats.join("empty.jsonl"),
            "{\"sessionId\":\"empty\",\"projectHash\":\"hash\"}\n",
        )
        .unwrap();

        assert!(list_from_config_dir(
            AgentCliKind::Gemini,
            &root.join("config"),
            &workdir
        )
            .unwrap()
            .is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detail_loader_applies_set_rewind_updates_and_tool_calls() {
        let root = test_root("detail");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let transcript = root.join("session.jsonl");
        let lines = [
            json!({
                "$set": {
                    "sessionId": "gemini-detail",
                    "summary": "初始标题",
                    "messages": [
                        {"id": "user-1", "type": "user", "content": "最初问题"},
                        {"id": "gemini-1", "type": "gemini", "content": "旧回答", "model": "gemini-2.5-pro"}
                    ]
                }
            }),
            json!({"id": "discarded", "type": "user", "content": "需要回退的消息"}),
            json!({"$rewindTo": "discarded"}),
            json!({"id": "user-2", "type": "user", "content": "最终问题", "timestamp": "2026-08-19T08:00:00Z"}),
            json!({
                "id": "gemini-2",
                "type": "gemini",
                "content": "工具执行完成",
                "model": "gemini-3-pro",
                "timestamp": "2026-08-19T08:01:00Z",
                "toolCalls": [{"id": "tool-1", "name": "read_file", "args": {"path": "src/App.vue"}, "result": "文件内容"}]
            }),
            json!({
                "$set": {
                    "summary": "最终标题",
                    "lastUpdated": "2026-08-19T08:02:00Z"
                }
            }),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&transcript, lines).unwrap();

        let (conversation, truncated) =
            load_conversation_limited(&transcript, TEST_LIMITS).unwrap();
        assert!(!truncated);
        assert_eq!(conversation.session_id.as_deref(), Some("gemini-detail"));
        assert_eq!(conversation.summary.as_deref(), Some("最终标题"));
        assert_eq!(conversation.messages.len(), 4);
        assert_eq!(conversation.messages[0].text.as_deref(), Some("最初问题"));
        assert_eq!(conversation.messages[1].text.as_deref(), Some("旧回答"));
        assert_eq!(conversation.messages[2].text.as_deref(), Some("最终问题"));
        assert_eq!(conversation.messages[3].text.as_deref(), Some("工具执行完成"));
        assert_eq!(conversation.messages[3].model.as_deref(), Some("gemini-3-pro"));
        assert_eq!(conversation.messages[3].tool_calls.len(), 1);
        assert_eq!(conversation.messages[3].tool_calls[0].name, "read_file");
        assert!(conversation.messages[3].tool_calls[0]
            .content
            .contains("src/App.vue"));
        assert!(!conversation
            .messages
            .iter()
            .any(|message| message.text.as_deref() == Some("需要回退的消息")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn search_uses_the_final_conversation_after_rewind() {
        let root = test_root("search-rewind");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let transcript = root.join("session.jsonl");
        let lines = [
            json!({"id": "kept-1", "type": "user", "content": "保留内容"}),
            json!({"id": "discarded", "type": "gemini", "content": "discarded-keyword"}),
            json!({"$rewindTo": "discarded"}),
            json!({"id": "kept-2", "type": "gemini", "content": "final-keyword"}),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&transcript, lines).unwrap();

        let request = |value: &str| SessionContentSearchRequest {
            terms: vec![SessionSearchTerm {
                index: 0,
                value: value.to_string(),
            }],
        };
        let discarded =
            search_conversation(&transcript, &request("discarded-keyword"), &|| true).unwrap();
        assert!(discarded.matched_term_indexes.is_empty());
        let kept = search_conversation(&transcript, &request("final-keyword"), &|| true).unwrap();
        assert_eq!(kept.matched_term_indexes, vec![0]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn search_excludes_tool_calls() {
        let root = test_root("tool-search");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let transcript = root.join("session.jsonl");
        let result = format!("{}gemini-tool-tail-keyword", "x".repeat(20 * 1024));
        fs::write(
            &transcript,
            json!({
                "id": "gemini-1",
                "type": "gemini",
                "content": "已调用工具",
                "toolCalls": [{
                    "id": "tool-1",
                    "name": "read_file",
                    "result": result
                }]
            })
            .to_string(),
        )
        .unwrap();
        let request = SessionContentSearchRequest {
            terms: vec![SessionSearchTerm {
                index: 0,
                value: "gemini-tool-tail-keyword".to_string(),
            }],
        };

        let search_result = search_conversation(&transcript, &request, &|| true).unwrap();

        assert!(search_result.matched_term_indexes.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn index_excludes_tool_calls_and_hidden_thoughts() {
        let root = test_root("index-filter");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let transcript = root.join("session.jsonl");
        fs::write(
            &transcript,
            json!({
                "id": "gemini-1",
                "type": "gemini",
                "content": [
                    {"type": "thinking", "text": "hidden-thinking-keyword"},
                    {"type": "text", "text": "visible answer"}
                ],
                "thoughts": [{"subject": "hidden-thought-keyword"}],
                "toolCalls": [{
                    "id": "tool-1",
                    "name": "read_file",
                    "result": "hidden-tool-keyword"
                }]
            })
            .to_string(),
        )
        .unwrap();

        let SessionIndexLoadResult::Updated { messages, .. } =
            index_conversation(&transcript, None, &|| true).unwrap()
        else {
            panic!("new source must be indexed");
        };
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "visible answer");
        fs::remove_dir_all(root).unwrap();
    }
