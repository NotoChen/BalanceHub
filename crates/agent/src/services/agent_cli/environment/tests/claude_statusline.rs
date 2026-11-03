//! Claude 2.1.270 settings-schema regressions through the production inventory
//! pipeline. File snapshots and installation discovery are fixture-only.
use super::*;
use crate::models::AgentEnvironmentInventory;
use serde_json::{json, Value};

fn command_statusline(extra: Value) -> Value {
    let mut value = json!({
        "type": "command",
        "command": "fixture-statusline-command"
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    value
}

fn build_statusline(name: &str, value: Option<Value>) -> (PathBuf, AgentEnvironmentInventory) {
    let settings = value.map_or_else(|| json!({}), |value| json!({ "statusLine": value }));
    let (installations, _) = FakeInstallationPort::new(vec![claude_fixture_installation(
        "2.1.270",
        AgentInstallationChannel::Stable,
    )]);
    let (root, _, inventory, _) = build_claude_inventory(
        name,
        [("settings", claude_json(settings))],
        [],
        [],
        &installations,
    );
    (root, inventory)
}

fn assert_statusline(inventory: &AgentEnvironmentInventory, valid_command: bool) {
    let declarations = claude_declarations(inventory, AgentAssetCategory::StatusUi, "status-line");
    assert_eq!(declarations.len(), 1, "{inventory:?}");
    let declaration_ids = [declarations[0].id.clone()];
    let (mode, declared, effective) = if valid_command {
        (
            AgentStatusUiMode::Command,
            AgentAssetDeclaredState::Enabled,
            AgentAssetState::Enabled,
        )
    } else {
        (
            AgentStatusUiMode::Unknown,
            AgentAssetDeclaredState::Unknown,
            AgentAssetState::Unknown,
        )
    };
    assert_status_asset_binding(
        inventory,
        StatusAssetExpectation {
            mode,
            declared,
            effective,
            resolution: AgentAssetResolutionRelation::Independent,
            terminal: None,
            contributors: &declaration_ids,
            represented: &declaration_ids,
            control_source: None,
            resolution_diagnostics: &[],
            mutation_reason: AgentAssetActionUnavailableReason::NoOfficialMechanism,
        },
    );
    let malformed = inventory
        .sources
        .iter()
        .flat_map(|source| &source.diagnostics)
        .filter(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::Malformed { .. }))
        .cloned()
        .collect::<Vec<_>>();
    let expected_malformed = if valid_command {
        Vec::new()
    } else {
        vec![AgentAssetDiagnostic::Malformed {
            format: AgentAssetDocumentFormat::Json,
            location: Some("statusLine".to_owned()),
        }]
    };
    assert_eq!(malformed, expected_malformed, "{inventory:?}");
    assert_no_structural_projection_diagnostics(inventory);
}

#[test]
fn claude_statusline_accepts_documented_command_shapes() {
    for (name, extra) in [
        ("required", json!({})),
        ("padding-zero", json!({ "padding": 0 })),
        ("padding-negative-fraction", json!({ "padding": -2.5 })),
        ("refresh-minimum", json!({ "refreshInterval": 1 })),
        ("refresh-fraction", json!({ "refreshInterval": 1.5 })),
        ("hide-vim", json!({ "hideVimModeIndicator": true })),
        ("show-vim", json!({ "hideVimModeIndicator": false })),
        (
            "all-documented-fields",
            json!({
                "padding": 2,
                "refreshInterval": 30,
                "hideVimModeIndicator": true
            }),
        ),
    ] {
        let (root, inventory) = build_statusline(
            &format!("claude-statusline-{name}"),
            Some(command_statusline(extra)),
        );
        assert_statusline(&inventory, true);
        remove_claude_fixture(root);
    }
}

#[test]
fn claude_statusline_rejects_invalid_required_and_strict_optional_fields() {
    for (name, value) in [
        ("empty-object", json!({})),
        ("missing-type", json!({ "command": "fixture-command" })),
        ("missing-command", json!({ "type": "command" })),
    ] {
        let (root, inventory) = build_statusline(&format!("claude-statusline-{name}"), Some(value));
        assert_statusline(&inventory, false);
        remove_claude_fixture(root);
    }
    for (field, invalid_values) in [
        (
            "type",
            vec![
                json!(null),
                json!(false),
                json!(7),
                json!([]),
                json!({}),
                json!("shell"),
            ],
        ),
        (
            "command",
            vec![json!(null), json!(false), json!(7), json!([]), json!({})],
        ),
        (
            "padding",
            vec![json!(null), json!(false), json!("2"), json!([]), json!({})],
        ),
        (
            "hideVimModeIndicator",
            vec![json!(null), json!(7), json!("false"), json!([]), json!({})],
        ),
    ] {
        for (index, invalid) in invalid_values.into_iter().enumerate() {
            let mut value = command_statusline(json!({}));
            value[field] = invalid;
            let (root, inventory) = build_statusline(
                &format!("claude-statusline-invalid-{field}-{index}"),
                Some(value),
            );
            assert_statusline(&inventory, false);
            remove_claude_fixture(root);
        }
    }
}

#[test]
fn claude_statusline_ignores_stripped_fields_and_caught_refresh_intervals() {
    for (index, refresh) in [
        json!(null),
        json!(false),
        json!(true),
        json!("30"),
        json!([]),
        json!({ "unexpected": "fixture-refresh-value" }),
        json!(0),
        json!(-1),
        json!(0.5),
    ]
    .into_iter()
    .enumerate()
    {
        let (root, inventory) = build_statusline(
            &format!("claude-statusline-refresh-fallback-{index}"),
            Some(command_statusline(json!({
                "refreshInterval": refresh,
                "syntheticExtension": { "nested": ["fixture-extra-value", null] }
            }))),
        );
        assert_statusline(&inventory, true);
        assert!(!serde_json::to_string(&inventory)
            .unwrap()
            .contains("fixture-extra-value"));
        remove_claude_fixture(root);
    }
}

#[test]
fn claude_statusline_distinguishes_missing_invalid_and_empty_command_configuration() {
    for (index, value) in [
        json!(null),
        json!(false),
        json!(true),
        json!(7),
        json!("command"),
        json!([]),
    ]
    .into_iter()
    .enumerate()
    {
        let (root, inventory) = build_statusline(
            &format!("claude-statusline-non-object-{index}"),
            Some(value),
        );
        assert_statusline(&inventory, false);
        remove_claude_fixture(root);
    }

    // Configuration validity does not assert execution or visible output.
    for (index, command) in ["", " ", "\t\n"].into_iter().enumerate() {
        let (root, inventory) = build_statusline(
            &format!("claude-statusline-empty-command-{index}"),
            Some(command_statusline(json!({ "command": command }))),
        );
        assert_statusline(&inventory, true);
        remove_claude_fixture(root);
    }

    let (root, inventory) = build_statusline("claude-statusline-absent", None);
    assert!(
        claude_declarations(&inventory, AgentAssetCategory::StatusUi, "status-line").is_empty()
    );
    assert!(claude_assets(&inventory, AgentAssetCategory::StatusUi, "status-line").is_empty());
    assert!(inventory
        .sources
        .iter()
        .flat_map(|source| &source.diagnostics)
        .all(|diagnostic| !matches!(diagnostic, AgentAssetDiagnostic::Malformed { .. })));
    assert_no_structural_projection_diagnostics(&inventory);
    remove_claude_fixture(root);
}

#[test]
fn claude_statusline_inventory_is_passive_and_redacts_executable_payloads() {
    let root = test_root("claude-statusline-passive");
    let marker = root.join("statusline-command-must-not-run");
    let command = format!(
        "echo statusline-command-private-marker > \"{}\"",
        marker.display()
    );
    assert!(!marker.exists());
    let (installations, _) = FakeInstallationPort::new(vec![claude_fixture_installation(
        "2.1.270",
        AgentInstallationChannel::Stable,
    )]);
    let (root, _, inventory, _) = build_claude_inventory_at_root(
        root,
        [(
            "settings",
            claude_json(json!({
                "statusLine": {
                    "type": "command",
                    "command": command,
                    "padding": 2,
                    "hideVimModeIndicator": true,
                    "refreshInterval": { "private": "statusline-refresh-private-marker" },
                    "syntheticExtension": { "private": "statusline-extra-private-marker" }
                }
            })),
        )],
        [],
        [],
        &installations,
    );
    assert_statusline(&inventory, true);
    assert!(
        !marker.exists(),
        "passive inventory executed a Status UI command"
    );
    let public = serde_json::to_string(&inventory).unwrap();
    for private in [
        "statusline-command-private-marker",
        "statusline-command-must-not-run",
        "statusline-refresh-private-marker",
        "statusline-extra-private-marker",
    ] {
        assert!(
            !public.contains(private),
            "Status UI payload leaked: {private}"
        );
    }
    remove_claude_fixture(root);
}
