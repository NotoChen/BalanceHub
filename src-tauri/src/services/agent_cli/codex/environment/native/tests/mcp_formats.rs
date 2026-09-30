use super::*;

#[test]
fn real_agent_plugin_mcp_enforces_its_format_without_losing_healthy_siblings() {
    let fixture = Fixture::new("agent-format");
    let root = fixture.plugin("agent@fixture", "local", "ignored-legacy");
    write(
        &root.join("plugin.json"),
        serde_json::to_vec(&serde_json::json!({
            "$schema": plugin::AGENT_SCHEMA, "name": "agent-space",
        }))
        .unwrap(),
    );
    skill(&root.join("skills/task/SKILL.md"), "task");
    skill(&root.join("skills/SKILL.md"), "root-is-not-a-direct-child");
    write(&root.join("mcp.json"), serde_json::to_vec(&serde_json::json!({
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "stdio": {"type": "stdio", "command": "./bin/server", "args": ["${PLUGIN_ROOT}/asset"],
                "cwd": "${PLUGIN_DATA}/cache", "env": {"TOKEN": PRIVATE_TOKEN}},
            "http": {"type": "streamable-http", "url": "https://fixture.invalid/mcp",
                "headers": {"Authorization": PRIVATE_TOKEN, "X-Fixture": PRIVATE_TOKEN}},
            "escape": {"type": "stdio", "command": "./../outside"},
            "reserved": {"type": "stdio", "command": PRIVATE_COMMAND, "env": {"PLUGIN_ROOT": "override"}},
            "null": {"type": "streamable-http", "url": "https://fixture.invalid", "headers": null},
            "insecure": {"type": "streamable-http", "url": "http://fixture.invalid"},
            "sse": {"type": "sse", "url": "https://fixture.invalid"},
            "unknown": {"type": "stdio", "command": PRIVATE_COMMAND, "enabled": false}
        }
    })).unwrap());
    let config = "[plugins.'agent@fixture']\n";
    let run = fixture.run::<false>(config);
    assert_eq!(run.inventory.assets.len(), 4);
    assert_eq!(run.rows(AgentAssetCategory::Mcp).len(), 2);
    assert_eq!(run.rows(AgentAssetCategory::Skill).len(), 1);
    let parent = run.row(AgentAssetCategory::Plugin, "agent@fixture");
    for name in ["stdio", "http"] {
        let row = run.row(AgentAssetCategory::Mcp, name);
        assert_eq!(row.effective_state, AgentAssetState::Enabled);
        assert_eq!(
            row.relationships.provided_by.as_deref(),
            Some(parent.stable_id.as_str())
        );
    }
    let http = run
        .capture
        .declarations
        .iter()
        .find(|asset| asset.category == AgentAssetCategory::Mcp && asset.native_id == "http")
        .unwrap();
    let AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginMcp { table }) =
        &http.native_payload
    else {
        panic!("native HTTP payload")
    };
    let headers = table.get("http_headers").unwrap().as_table().unwrap();
    assert_eq!(headers.len(), 1);
    assert!(headers.contains_key("X-Fixture"));
    write(
        &root.join("mcp.json"),
        r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{},"unknown":true}"#,
    );
    let invalid_top_level = fixture.run::<false>(config);
    assert_eq!(invalid_top_level.inventory.assets.len(), 2);
    assert_eq!(invalid_top_level.rows(AgentAssetCategory::Mcp).len(), 0);
}

#[test]
fn legacy_mcp_normalization_matches_native_oauth_cwd_and_optional_nulls() {
    let fixture = Fixture::new("legacy-normalization");
    let root = fixture.plugin("legacy@fixture", "local", "legacy");
    let value = serde_json::json!({
        "type": "unknown-native-type-is-advisory", "url": "https://fixture.invalid",
        "oauth": {"clientId": "camel", "client_id": "snake", "callbackUrl": "https://fixture.invalid/callback", "callbackPort": 0},
        "headers": {"Not-Natively-Translated": PRIVATE_TOKEN}, "enabled": false, "required": null,
        "ignoredExtension": {"any": [null, 18446744073709551615_u64]},
    });
    let (table, decoded) = mcp::normalize(
        value,
        &root,
        &fixture.home.join(".codex"),
        "legacy@fixture",
        false,
    )
    .unwrap();
    assert!(!decoded.enabled);
    assert_eq!(decoded.transport, AgentMcpTransport::Http);
    assert!(table.get("http_headers").is_none());
    assert!(table.get("type").is_none());
    let oauth = table.get("oauth").unwrap().as_table().unwrap();
    assert_eq!(
        oauth.get("client_id").and_then(toml::Value::as_str),
        Some("snake")
    );
    assert_eq!(
        oauth.get("callback_port").and_then(toml::Value::as_integer),
        Some(0)
    );
    let (table, decoded) = mcp::normalize(
        serde_json::json!({"command": PRIVATE_COMMAND, "cwd": "helpers", "oauth": false}),
        &root,
        &fixture.home.join(".codex"),
        "legacy@fixture",
        false,
    )
    .unwrap();
    assert_eq!(decoded.transport, AgentMcpTransport::Stdio);
    assert_eq!(
        Path::new(table.get("cwd").unwrap().as_str().unwrap()),
        root.join("helpers")
    );
    assert!(table.get("oauth").is_none());
    for value in [
        serde_json::json!({"command": "runner", "url": "https://fixture.invalid"}),
        serde_json::json!({"command": "runner", "enabled": "false"}),
        serde_json::json!({"command": "runner", "args": [1]}),
    ] {
        assert!(mcp::normalize(
            value,
            &root,
            &fixture.home.join(".codex"),
            "legacy@fixture",
            false
        )
        .is_none());
    }
}

#[test]
fn agent_mcp_rejects_case_colliding_headers_paths_and_unsupported_schemas() {
    let fixture = Fixture::new("agent-validation");
    let root = fixture.plugin("agent@fixture", "local", "agent");
    for value in [
        serde_json::json!({"type": "streamable-http", "url": "https://fixture.invalid", "headers": {"X-Test": "a", "x-test": "b"}}),
        serde_json::json!({"type": "streamable-http", "url": "https://fixture.invalid/#fragment"}),
        serde_json::json!({"type": "streamable-http", "url": "https://user@fixture.invalid"}),
        serde_json::json!({"type": "streamable-http", "url": "https://fixture.invalid", "headers": {"X-Test": "line\nbreak"}}),
        serde_json::json!({"type": "stdio", "command": "../outside"}),
        serde_json::json!({"type": "stdio", "command": "runner", "cwd": "${PLUGIN_DATA}/../../../../outside"}),
        serde_json::json!({"type": "stdio", "command": "runner", "cwd": null}),
    ] {
        assert!(mcp::normalize(
            value,
            &root,
            &fixture.home.join(".codex"),
            "agent@fixture",
            true
        )
        .is_none());
    }
    for host in ["localhost", "127.0.0.1", "[::1]"] {
        let valid =
            serde_json::json!({"type":"streamable-http", "url":format!("http://{host}/mcp")});
        assert!(mcp::normalize(
            valid,
            &root,
            &fixture.home.join(".codex"),
            "agent@fixture",
            true
        )
        .is_some());
    }
    assert!(mcp::file_servers(serde_json::json!({"$schema": "https://agent-plugins.org/schemas/2.0.0/mcp.schema.json", "mcpServers": {}}), true).is_none());
    assert!(mcp::file_servers(serde_json::json!({"mcpServers": {}}), true).is_none());
}

#[test]
fn codex_skill_frontmatter_native_fallback_and_scalar_repair_are_bounded() {
    assert_eq!(
        super::super::skill::validate_skill(
            "---\nname: '  Two   Words  '\ndescription: useful\n---\n",
            "folder"
        ),
        Some("Two Words".to_owned())
    );
    assert_eq!(
        super::super::skill::validate_skill(
            "---\nname: ''\ndescription: Use: this tool\n---\n",
            "fallback"
        ),
        Some("fallback".to_owned())
    );
    for invalid in [
        "description: prose",
        "---\nname: only\n---",
        "---\ndescription: ''\n---",
        "---\ndescription: text",
    ] {
        assert!(super::super::skill::validate_skill(invalid, "fallback").is_none());
    }
    let valid_unicode = format!("---\nname: {}\ndescription: text\n---", "技".repeat(64));
    let invalid_unicode = format!("---\nname: {}\ndescription: text\n---", "技".repeat(65));
    assert!(super::super::skill::validate_skill(&valid_unicode, "fallback").is_some());
    assert!(super::super::skill::validate_skill(&invalid_unicode, "fallback").is_none());
}
