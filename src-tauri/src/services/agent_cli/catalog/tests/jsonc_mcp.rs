use super::*;
use crate::services::agent_cli::{
    catalog::hook_tests::{adopt, assert_verified, edit_request, plan, run},
    config_support::parse_jsonc_document,
};
use serde_json::json;

#[test]
fn gemini_existing_commented_mcp_can_be_adopted_and_updated_without_rewriting_neighbors() {
    let (_temporary, inspector, service) = fixture();
    let path = inspector.home.join(".gemini/settings.json");
    let original = r#"{
  // User configuration stays readable and keeps its notes.
  "mcpServers": {
    "user-tool": {"command":"fixture-runner","args":["v1"],"env":{"TOKEN":"synthetic-private-token"}},
    /* Unselected server: preserve its exact text. */
    "neighbor": { "command": "untouched-runner", "args": ["keep"] }
  },
  "hooks": {"BeforeTool":[{"hooks":[{"type":"command","command":"printf do-not-run"}]}]},
  "unknownSetting": { "url": "https://example.invalid/path//value", "enabled": true }
}
"#;
    fs::write(&path, original).unwrap();
    let initial = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let binding = initial
        .assets
        .iter()
        .flat_map(|asset| &asset.bindings)
        .find(|binding| {
            binding.native.agent_kind == AgentCliKind::Gemini
                && binding.native.category == AgentAssetCategory::Mcp
                && binding.native.native_id == "user-tool"
        })
        .expect("the actual native parser must discover the commented MCP");
    assert!(binding.can_adopt, "{:?}", binding.reason);
    let saved = adopt(&service, inspector.clone(), &binding.id);
    assert!(serde_json::to_string(&saved)
        .unwrap()
        .contains("synthetic-private-token"));
    let mut edit = edit_request(&saved);
    edit.mcp.as_mut().unwrap().args = vec!["v2".to_owned()];
    service.save(edit).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let target = catalog
        .targets
        .iter()
        .find(|target| {
            target.agent_kind == AgentCliKind::Gemini
                && target.scope == AgentAssetScope::User
                && target.categories == [AgentAssetCategory::Mcp]
        })
        .expect("the exact user configuration remains a writable destination");
    let operation = run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::ApplyDefinition,
            vec![target.id.clone()],
        ),
    );
    assert_eq!(operation.targets.len(), 1);
    assert_verified(&operation);
    let after = fs::read_to_string(&path).unwrap();
    for retained in [
        "// User configuration stays readable and keeps its notes.",
        "/* Unselected server: preserve its exact text. */",
        "\"neighbor\": { \"command\": \"untouched-runner\", \"args\": [\"keep\"] }",
        "\"unknownSetting\": { \"url\": \"https://example.invalid/path//value\", \"enabled\": true }",
    ] {
        assert!(after.contains(retained), "untouched source text was rewritten");
    }
    let before = parse_jsonc_document(original).unwrap();
    let after = parse_jsonc_document(&after).unwrap();
    assert_eq!(after["mcpServers"]["user-tool"]["args"], json!(["v2"]));
    assert_eq!(
        after["mcpServers"]["user-tool"]["env"],
        before["mcpServers"]["user-tool"]["env"]
    );
    assert_eq!(
        after["mcpServers"]["neighbor"],
        before["mcpServers"]["neighbor"]
    );
    assert_eq!(after["hooks"], before["hooks"]);
    let refreshed = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let item = refreshed
        .assets
        .iter()
        .find(|item| item.id == saved.asset_id)
        .unwrap();
    assert_eq!(item.bindings.len(), 1);
    assert_eq!(item.bindings[0].drift, AgentCatalogDrift::InSync);
}

#[test]
fn mcp_comments_are_native_specific_and_do_not_accept_ambiguous_json() {
    let definition = native::GEMINI
        .decode(&json!({"command":"fixture-runner","args":[]}))
        .unwrap();
    let valid = b"{ /* native Gemini comment */ \"mcpServers\": {} }";
    assert!(native::GEMINI
        .patch_mcp(Some(valid), "user-tool", &definition)
        .is_ok());
    assert!(native::CLAUDE
        .patch_mcp(Some(valid), "user-tool", &definition)
        .is_err());
    for invalid in [
        "{\"mcpServers\":{},\"mcpServers\":{}}",
        "{\"mcpServers\":{},}",
        "{/* unterminated comment",
    ] {
        assert!(native::GEMINI
            .patch_mcp(Some(invalid.as_bytes()), "user-tool", &definition)
            .is_err());
    }
}
