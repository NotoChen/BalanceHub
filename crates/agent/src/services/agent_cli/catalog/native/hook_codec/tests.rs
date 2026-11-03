use super::*;
use serde_json::json;

fn validate(definition: &HookNativeDefinition) -> Result<(), String> {
    selected_handler(definition).map(|_| ())
}

fn document(text: &str, format: HookDocumentFormat) -> HookDocument {
    HookDocument::from_bytes("fixture", Some(text.as_bytes().to_vec()), format).unwrap()
}

fn rule(document: &HookDocument, event: &str, group: usize, handler: usize) -> HookNativeRule {
    let original = document.root["hooks"][event][group].clone();
    let mut selected = original.clone();
    selected["hooks"] = json!([original["hooks"][handler]]);
    HookNativeRule {
        source_id: document.source_id.clone(),
        native_asset_id: Some(format!("fixture:{event}:{group}:{handler}")),
        definition: HookNativeDefinition {
            event: event.to_owned(),
            group: selected,
        },
        anchor: HookNativeAnchor {
            event: event.to_owned(),
            group_index: group,
            handler_index: handler,
            original_group: original,
        },
        enabled: AgentAssetDeclaredState::Enabled,
    }
}

fn location(event: &str, group: usize, handler: usize) -> HookLocation {
    HookLocation {
        event: event.to_owned(),
        group,
        handler,
    }
}

#[test]
fn same_group_batch_keeps_siblings_and_tracks_their_original_positions() {
    let text = r#"{
      "hooks":{"Start":[{"matcher":"x", "unknown":true, "hooks":[
        {"type":"command","command":"first"},
        // the middle rule keeps its bytes
        { "type": "command", "command": "middle", "metadata": [1, 2] },
        {"type":"command","command":"last"}
      ]}]},
      "preferences": { "theme": "system" }
    }"#;
    let document = document(text, HookDocumentFormat::Jsonc);
    let first = rule(&document, "Start", 0, 0);
    let last = rule(&document, "Start", 0, 2);
    let mut replacement = last.definition.clone();
    replacement.group["hooks"][0]["command"] = json!("updated-last");
    let edited = edit_document_tracked(
        &document,
        &[
            HookNativeEdit::Remove { original: first },
            HookNativeEdit::Replace {
                original: last,
                definition: replacement,
            },
        ],
        validate,
    )
    .unwrap();
    assert!(edited.locations[&location("Start", 0, 0)].is_none());
    assert!(edited.locations[&location("Start", 0, 1)] == Some(location("Start", 0, 0)));
    assert!(edited.locations[&location("Start", 0, 2)] == Some(location("Start", 0, 1)));
    assert_eq!(
        edited.root["hooks"]["Start"][0]["hooks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(edited.root["hooks"]["Start"][0]["unknown"], true);
    let rendered =
        String::from_utf8(document.render(&edited.root).unwrap().unwrap().bytes).unwrap();
    assert!(rendered.contains("// the middle rule keeps its bytes"));
    assert!(rendered.contains(r#"{ "type": "command", "command": "middle", "metadata": [1, 2] }"#));
    assert!(rendered.contains(r#""preferences": { "theme": "system" }"#));
}

#[test]
fn metadata_change_carries_identity_without_lending_it_to_an_identical_sibling() {
    let document = document(
        r#"{"hooks":{"Start":[{"hooks":[{"command":"first"},{"command":"same"},{"command":"same"}]}]}}"#,
        HookDocumentFormat::Json,
    );
    let first = rule(&document, "Start", 0, 0);
    let mut replacement = first.definition.clone();
    replacement.event = "End".to_owned();
    let result = edit_document_tracked(
        &document,
        &[HookNativeEdit::Replace {
            original: first,
            definition: replacement,
        }],
        validate,
    )
    .unwrap();
    assert!(result.locations[&location("Start", 0, 0)] == Some(location("End", 0, 0)));
    assert!(result.locations[&location("Start", 0, 1)] == Some(location("Start", 0, 0)));
    assert!(result.locations[&location("Start", 0, 2)] == Some(location("Start", 0, 1)));
    let duplicate = rule(&document, "Start", 0, 1);
    assert!(edit_document(
        &document,
        &[HookNativeEdit::Remove {
            original: duplicate
        }],
        validate
    )
    .is_err());
}

#[test]
fn replace_cannot_merge_two_native_policy_histories() {
    let document = document(
        r#"{"hooks":{"Start":[{"hooks":[{"command":"first"}]}],"End":[{"hooks":[{"command":"first"}]}]}}"#,
        HookDocumentFormat::Json,
    );
    let original = rule(&document, "Start", 0, 0);
    let mut replacement = original.definition.clone();
    replacement.event = "End".to_owned();
    assert!(edit_document_tracked(
        &document,
        &[HookNativeEdit::Replace {
            original,
            definition: replacement
        }],
        validate
    )
    .is_err());
}

#[test]
fn restore_is_idempotent_and_refuses_reordered_neighbor_evidence() {
    let document = document(
        r#"{"hooks":{"Start":[{"hooks":[{"command":"before"},{"command":"selected"},{"command":"after"}]}]}}"#,
        HookDocumentFormat::Json,
    );
    let selected = rule(&document, "Start", 0, 1);
    let (removed, _) = edit_document(
        &document,
        &[HookNativeEdit::Remove {
            original: selected.clone(),
        }],
        validate,
    )
    .unwrap();
    let removed = HookDocument::from_bytes(
        "fixture",
        Some(serde_json::to_vec(&removed).unwrap()),
        HookDocumentFormat::Json,
    )
    .unwrap();
    let restore = HookNativeEdit::Restore {
        original: selected.clone(),
        definition: selected.definition.clone(),
    };
    let (restored, _) = edit_document(&removed, std::slice::from_ref(&restore), validate).unwrap();
    assert_eq!(restored, document.root);
    assert_eq!(
        edit_document(&document, std::slice::from_ref(&restore), validate)
            .unwrap()
            .0,
        document.root
    );
    let mut changed = removed.root.clone();
    changed["hooks"]["Start"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let changed = HookDocument::from_bytes(
        "fixture",
        Some(serde_json::to_vec(&changed).unwrap()),
        HookDocumentFormat::Json,
    )
    .unwrap();
    assert!(edit_document(&changed, &[restore], validate).is_err());
}

#[test]
fn source_evidence_and_json_duplicate_keys_are_never_guessed() {
    assert!(HookDocument::from_bytes(
        "fixture",
        Some(br#"{"hooks":{},"hooks":{}}"#.to_vec()),
        HookDocumentFormat::Json
    )
    .is_err());
    for text in [
        r#"{"hooks":{},}"#,
        r#"{"hooks":{"Start":[{"hooks":[{},]}]}}"#,
    ] {
        assert!(HookDocument::from_bytes(
            "fixture",
            Some(text.as_bytes().to_vec()),
            HookDocumentFormat::Jsonc
        )
        .is_err());
    }
    let document = document(
        r#"{"hooks":{"Start":[{"hooks":[{"command":"only"}]}]}}"#,
        HookDocumentFormat::Json,
    );
    let mut original = rule(&document, "Start", 0, 0);
    original.source_id = "other-source".to_owned();
    assert!(edit_document(&document, &[HookNativeEdit::Remove { original }], validate).is_err());
}

#[test]
fn toml_inline_and_array_of_tables_edits_keep_unrelated_text_and_comments() {
    for text in [
        "# fixture header\n[hooks]\nStart = [{ matcher = 'x', hooks = [\n  { type = 'command', command = 'first' },\n  # retain second rule\n  { type = 'command', command = 'second', custom = { a = 1 } },\n] }]\n[appearance]\ntheme = 'system' # retain appearance\n",
        "# fixture header\n[[hooks.Start]]\nmatcher = 'x'\n[[hooks.Start.hooks]]\ntype = 'command'\ncommand = 'first'\n# retain second rule\n[[hooks.Start.hooks]]\ntype = 'command'\ncommand = 'second'\ncustom = { a = 1 }\n[appearance]\ntheme = 'system' # retain appearance\n",
    ] {
        let document = document(text, HookDocumentFormat::Toml);
        let original = rule(&document, "Start", 0, 0);
        let mut definition = original.definition.clone();
        definition.group["hooks"][0]["command"] = json!("updated");
        let (root, _) = edit_document(&document, &[HookNativeEdit::Replace { original, definition }], validate).unwrap();
        let rendered = String::from_utf8(document.render(&root).unwrap().unwrap().bytes).unwrap();
        assert!(rendered.contains("# fixture header"));
        assert!(rendered.contains("# retain second rule"), "{rendered}");
        assert!(rendered.contains("command = 'second'"), "{rendered}");
        assert!(rendered.contains("custom = { a = 1 }"), "{rendered}");
        assert!(rendered.contains("theme = 'system' # retain appearance"));
        assert_eq!(config_document::parse(rendered.as_bytes(), ConfigDocumentFormat::Toml), Some(root));
    }
}

#[test]
fn toml_native_hook_state_lifecycle_retains_empty_policy_table() {
    let text = "# fixture header\n[features]\nhooks = true # retain feature flag\n";
    let mut current = document(text, HookDocumentFormat::Toml);
    let key = "/synthetic/home/.codex/hooks.json:pre_tool_use:1:0";
    for state in [
        json!({(key): {"enabled": false}}),
        json!({(key): {"enabled": true}}),
        json!({}),
    ] {
        let mut desired = current.root.clone();
        desired["hooks"] = json!({"state": state});
        let rendered = current.render(&desired).unwrap().unwrap().bytes;
        let text = std::str::from_utf8(&rendered).unwrap();
        assert!(text.contains("# fixture header"));
        assert!(text.contains("hooks = true # retain feature flag"));
        assert_eq!(
            config_document::parse(&rendered, ConfigDocumentFormat::Toml),
            Some(desired)
        );
        current =
            HookDocument::from_bytes("fixture", Some(rendered), HookDocumentFormat::Toml).unwrap();
    }
    assert_eq!(current.root["hooks"]["state"], json!({}));
    assert!(current.render(&current.root).unwrap().is_none());
}

#[test]
fn toml_removing_final_hook_state_preserves_empty_parent_and_unrelated_source() {
    for text in [
        "# fixture header\n[hooks.state.selected]\nenabled = true\ntrusted_hash = 'selected-trust'\n[hooks.state.neighbor]\nenabled = false # retain neighbor\ntrusted_hash = 'neighbor-trust'\n[appearance]\ntheme = 'system' # retain appearance\n",
        "# fixture header\nhooks.state.selected.enabled = true\nhooks.state.selected.trusted_hash = 'selected-trust'\nhooks.state.neighbor.enabled = false # retain neighbor\nhooks.state.neighbor.trusted_hash = 'neighbor-trust'\n[appearance]\ntheme = 'system' # retain appearance\n",
        "# fixture header\n[hooks]\nstate = { selected = { enabled = true, trusted_hash = 'selected-trust' }, neighbor = { enabled = false, trusted_hash = 'neighbor-trust' } } # retain neighbor\n[appearance]\ntheme = 'system' # retain appearance\n",
    ] {
        let mut current = document(text, HookDocumentFormat::Toml);
        for key in ["selected", "neighbor"] {
            let mut desired = current.root.clone();
            desired["hooks"]["state"]
                .as_object_mut()
                .unwrap()
                .remove(key);
            let rendered = current.render(&desired).unwrap().unwrap().bytes;
            let text = std::str::from_utf8(&rendered).unwrap();
            assert!(text.contains("theme = 'system' # retain appearance"));
            if key == "selected" {
                assert!(text.contains("# retain neighbor"));
                assert!(text.contains("trusted_hash = 'neighbor-trust'"));
                assert_eq!(desired["hooks"]["state"]["neighbor"]["enabled"], false);
            }
            assert_eq!(
                config_document::parse(&rendered, ConfigDocumentFormat::Toml),
                Some(desired)
            );
            current =
                HookDocument::from_bytes("fixture", Some(rendered), HookDocumentFormat::Toml)
                    .unwrap();
        }
        assert_eq!(current.root["hooks"]["state"], json!({}));
    }
}
