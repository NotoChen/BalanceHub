mod feature;

use super::*;
use crate::services::agent_cli::codex::environment::hooks::{self as schema, CodexHookPayload};
use serde_json::json;

fn hook_file(command: &str) -> Value {
    json!({"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":command}]}]}})
}

fn trusted_state(key: &str, file: &Value) -> String {
    let group = &file["hooks"]["SessionStart"][0];
    let hash = schema::normalized_hash("SessionStart", group, &group["hooks"][0]).unwrap();
    format!(
        "[hooks.state.{}]\ntrusted_hash={hash:?}\n",
        toml::Value::String(key.to_owned())
    )
}

#[test]
fn standalone_hook_policy_keeps_unknown_input_distinct_from_an_enabled_default() {
    let fixture = Fixture::new("hook-user-state");
    let file = hook_file("bh-standalone");
    let path = fixture.home.join(".codex/hooks.json");
    write(&path, serde_json::to_vec(&file).unwrap());
    let key = format!("{}:session_start:0:0", path.display());
    let trust = trusted_state(&key, &file);
    for (config, declared, effective, record_trust) in [
        (
            String::new(),
            AgentAssetState::Enabled,
            AgentAssetState::Unknown,
            AgentTrustState::Untrusted,
        ),
        (
            trust.clone(),
            AgentAssetState::Enabled,
            AgentAssetState::Enabled,
            AgentTrustState::Trusted,
        ),
        (
            format!("{trust}enabled=false\n"),
            AgentAssetState::Disabled,
            AgentAssetState::Disabled,
            AgentTrustState::Trusted,
        ),
        (
            "[".to_owned(),
            AgentAssetState::Unknown,
            AgentAssetState::Unknown,
            AgentTrustState::Unknown,
        ),
    ] {
        let run = fixture.run_with_workspace::<false>(&config, false);
        assert_eq!(run.rows(AgentAssetCategory::Hook).len(), 1);
        let row = run.row(AgentAssetCategory::Hook, &key);
        assert_eq!(row.declared_state, declared);
        assert_eq!(row.effective_state, effective);
        assert_eq!(row.trust_state, record_trust);
        assert_eq!(row.represented_declaration_ids.len(), 1);
        assert_eq!(row.resolution.contributor_ids.len(), 1);
        let reversed = fixture.run_with_workspace::<true>(&config, false);
        assert_eq!(run.normalized_rows(), reversed.normalized_rows());
    }
}

#[test]
fn native_hook_regex_rejects_a_bare_star_without_dropping_healthy_siblings() {
    let fixture = Fixture::new("hook-regex");
    for (matcher, expected_count) in [("*", None), (".*", Some(2))] {
        write(&fixture.home.join(".codex/hooks.json"), serde_json::to_vec(&json!({
            "hooks": {
                "SessionStart": [{"matcher": matcher, "hooks": [{"type":"command","command":"bh-regex"}]}],
                "SessionEnd": [{"hooks": [{"type":"command","command":"bh-valid-sibling"}]}],
            }
        })).unwrap());
        let run = fixture.run_with_workspace::<false>("", false);
        assert_eq!(
            run.rows(AgentAssetCategory::Hook).len(),
            if matcher == "*" { 1 } else { 2 }
        );
        assert_eq!(run.inventory.hook_rule_counts[0].rule_count, expected_count);
        assert!(run
            .rows(AgentAssetCategory::Hook)
            .iter()
            .any(|row| row.native_id.ends_with(":session_end:0:0")));
    }
}

#[test]
fn plugin_hook_paths_replace_the_default_and_keep_exact_native_keys() {
    let fixture = Fixture::new("hook-files");
    let root = fixture.plugin("hooked@fixture", "1.0.0", "hooked");
    let default = hook_file("bh-default");
    write(
        &root.join("hooks/hooks.json"),
        serde_json::to_vec(&default).unwrap(),
    );
    let config = "[plugins.'hooked@fixture']\nenabled=true\n";
    let before = fixture.run::<false>(config);
    assert_eq!(before.rows(AgentAssetCategory::Hook).len(), 1);
    let native_id = "hooked@fixture:hooks/hooks.json:session_start:0:0";
    let row = before.row(AgentAssetCategory::Hook, native_id);
    assert_eq!(row.declared_state, AgentAssetState::Enabled);
    assert_eq!(row.trust_state, AgentTrustState::Untrusted);
    assert_eq!(row.effective_state, AgentAssetState::Unknown);
    assert!(!row.writable);
    let parent = before.row(AgentAssetCategory::Plugin, "hooked@fixture");
    assert_eq!(
        row.relationships.provided_by.as_deref(),
        Some(parent.stable_id.as_str())
    );
    assert_eq!(
        row.relationships.action_owner.as_deref(),
        Some(parent.stable_id.as_str())
    );

    write(
        &root.join(".codex-plugin/plugin.json"),
        serde_json::to_vec(&json!({"name":"hooked","hooks":["./alternate.json"]})).unwrap(),
    );
    write(
        &root.join("alternate.json"),
        serde_json::to_vec(&hook_file("bh-alternate")).unwrap(),
    );
    let after = fixture.run::<false>(config);
    assert_eq!(after.rows(AgentAssetCategory::Hook).len(), 1);
    after.row(
        AgentAssetCategory::Hook,
        "hooked@fixture:alternate.json:session_start:0:0",
    );
    assert!(!after
        .capture
        .sources
        .iter()
        .any(|source| source.path == root.join("hooks/hooks.json")));
    assert_eq!(after.inventory.hook_rule_counts[0].rule_count, Some(1));
}

#[test]
fn inline_plugin_hook_trust_and_parent_enablement_are_independent() {
    let fixture = Fixture::new("hook-inline");
    let root = fixture.plugin("hooked@fixture", "local", "hooked");
    let first = hook_file("bh-first");
    let second = hook_file("bh-second");
    write(
        &root.join(".codex-plugin/plugin.json"),
        serde_json::to_vec(&json!({"name":"hooked","hooks":[first,second]})).unwrap(),
    );
    let first_key = "hooked@fixture:plugin.json#hooks[0]:session_start:0:0";
    let second_key = "hooked@fixture:plugin.json#hooks[1]:session_start:0:0";
    let mut identity = None;
    for enabled in [true, false] {
        let config = format!(
            "[plugins.'hooked@fixture']\nenabled={enabled}\n{}",
            trusted_state(first_key, &first)
        );
        let run = fixture.run::<false>(&config);
        assert_eq!(run.rows(AgentAssetCategory::Hook).len(), 2);
        let trusted = run.row(AgentAssetCategory::Hook, first_key);
        let untrusted = run.row(AgentAssetCategory::Hook, second_key);
        assert_eq!(trusted.declared_state, AgentAssetState::Enabled);
        assert_eq!(untrusted.declared_state, AgentAssetState::Enabled);
        assert_eq!(trusted.trust_state, AgentTrustState::Trusted);
        assert_eq!(untrusted.trust_state, AgentTrustState::Untrusted);
        assert_eq!(
            trusted.effective_state,
            if enabled {
                AgentAssetState::Enabled
            } else {
                AgentAssetState::Disabled
            }
        );
        assert_eq!(
            untrusted.effective_state,
            if enabled {
                AgentAssetState::Unknown
            } else {
                AgentAssetState::Disabled
            }
        );
        assert_eq!(trusted.inspection_source_id, untrusted.inspection_source_id);
        if let Some(previous) = &identity {
            assert_eq!(previous, &trusted.stable_id);
        }
        identity = Some(trusted.stable_id.clone());
        let reversed = fixture.run::<true>(&config);
        assert_eq!(run.normalized_rows(), reversed.normalized_rows());
    }
}

#[test]
fn plugin_hook_assessment_rejects_forged_hash_position_parent_and_inline_content() {
    let fixture = Fixture::new("hook-proof");
    let root = fixture.plugin("hooked@fixture", "local", "hooked");
    let file = hook_file("bh-hook");
    write(
        &root.join(".codex-plugin/plugin.json"),
        serde_json::to_vec(&json!({"name":"hooked","hooks":file})).unwrap(),
    );
    let key = "hooked@fixture:plugin.json#hooks[0]:session_start:0:0";
    let run = fixture.run::<false>(&format!(
        "[plugins.'hooked@fixture']\nenabled=true\n{}",
        trusted_state(key, &file)
    ));
    assert_eq!(run.rows(AgentAssetCategory::Hook).len(), 1);
    for mutation in 0..5 {
        let mut declarations = run.capture.declarations.clone();
        let asset = declarations
            .iter_mut()
            .find(|asset| asset.native_id == key)
            .unwrap();
        let AgentAssetNativePayload::CodexHook(CodexHookPayload::Definition {
            current_hash,
            evidence,
            ..
        }) = &mut asset.native_payload
        else {
            panic!("wrong fixture payload")
        };
        match mutation {
            0 => *current_hash = "sha256:forged".to_owned(),
            1 => evidence.group_index += 1,
            2 => evidence
                .plugin
                .as_mut()
                .unwrap()
                .manifest_key
                .push_str("-other"),
            3 => {
                evidence.group["hooks"][0]["command"] = "bh-modified".into();
                *current_hash = schema::normalized_hash(
                    "SessionStart",
                    &evidence.group,
                    &evidence.group["hooks"][0],
                )
                .unwrap();
            }
            _ => asset.trust_state = AgentTrustState::Trusted,
        }
        assert!(matches!(
            run.assess(&declarations, AgentAssetCategory::Hook, key),
            AgentAssetAssessmentResult::Unsupported(_)
        ));
    }
}

#[test]
fn local_agent_plugin_format_never_advertises_executor_hooks_as_local() {
    let fixture = Fixture::new("hook-agent-format");
    let root = fixture.plugin("hooked@fixture", "local", "hooked");
    write(
        &root.join("plugin.json"),
        serde_json::to_vec(&json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name":"hooked",
            "extensions":{"com.openai":{"hooks":hook_file("bh-executor-only")}}
        }))
        .unwrap(),
    );
    write(
        &root.join("hooks/hooks.json"),
        serde_json::to_vec(&hook_file("bh-not-local")).unwrap(),
    );
    let run = fixture.run::<false>("[plugins.'hooked@fixture']\nenabled=true\n");
    assert_eq!(run.rows(AgentAssetCategory::Plugin).len(), 1);
    assert!(run.rows(AgentAssetCategory::Hook).is_empty());
    assert_eq!(run.inventory.hook_rule_counts[0].rule_count, Some(0));
    assert!(!run
        .capture
        .sources
        .iter()
        .any(|source| source.path == root.join("hooks/hooks.json")));
}

#[test]
fn bundled_cleanup_exception_is_exact_and_does_not_grant_nearby_plugin_hooks() {
    let fixture = Fixture::new("hook-builtin");
    let mut config = String::new();
    for (id, matcher, builtin) in [
        ("browser@openai-bundled", None, true),
        ("chrome@openai-bundled", Some("*"), false),
        ("browser@fixture", None, false),
    ] {
        let root = fixture.plugin(id, "local", "browser");
        let mut group =
            json!({"hooks":[{"type":"mcp_tool","server":"node_repl","tool":"turn_ended"}]});
        if let Some(matcher) = matcher {
            group["matcher"] = matcher.into();
        }
        write(
            &root.join("hooks/hooks.json"),
            serde_json::to_vec(&json!({"hooks":{"Stop":[group]}})).unwrap(),
        );
        let key = format!("{id}:hooks/hooks.json:stop:0:0");
        config.push_str(&format!(
            "[plugins.{id:?}]\nenabled=true\n[hooks.state.{key:?}]\nenabled=false\n"
        ));
        let run = fixture.run::<false>(&config);
        let row = run.row(AgentAssetCategory::Hook, &key);
        assert_eq!(
            row.declared_state,
            if builtin {
                AgentAssetState::Enabled
            } else {
                AgentAssetState::Disabled
            }
        );
        assert_eq!(
            row.effective_state,
            if builtin {
                AgentAssetState::Enabled
            } else {
                AgentAssetState::Disabled
            }
        );
        assert_eq!(
            row.trust_state,
            if builtin {
                AgentTrustState::Trusted
            } else {
                AgentTrustState::Untrusted
            }
        );
    }
}

#[test]
fn hook_hash_uses_normalized_native_defaults_and_keeps_security_relevant_changes() {
    let group = json!({"matcher":"ignored-on-stop"});
    let bare = json!({"type":"command","command":"bh-command"});
    let explicit = json!({"type":"command","command":"bh-command","async":false,"timeout":600,"additionalContextLimit":2500});
    let hash = |event: &str, group: &Value, handler: &Value| {
        schema::normalized_hash(event, group, handler).unwrap()
    };
    assert_eq!(
        hash("SessionStart", &group, &bare),
        hash("SessionStart", &group, &explicit)
    );
    assert_eq!(hash("Stop", &group, &bare), hash("Stop", &json!({}), &bare));
    assert_ne!(
        hash("PreToolUse", &group, &bare),
        hash("PreToolUse", &json!({}), &bare)
    );
    let mut clamped = bare.clone();
    clamped["timeout"] = 99.into();
    let mut max_timeout = bare.clone();
    max_timeout["timeout"] = 3.into();
    assert_eq!(
        hash("SessionEnd", &group, &clamped),
        hash("SessionEnd", &group, &max_timeout)
    );
    max_timeout["async"] = true.into();
    assert_ne!(
        hash("SessionEnd", &group, &clamped),
        hash("SessionEnd", &group, &max_timeout)
    );
    assert_eq!(
        hash(
            "Stop",
            &group,
            &json!({"type":"mcp_tool","server":"s","tool":"t","input":{"z":1,"a":{"z":2,"a":3}}})
        ),
        hash(
            "Stop",
            &group,
            &json!({"type":"mcp_tool","server":"s","tool":"t","input":{"a":{"a":3,"z":2},"z":1}})
        )
    );
}
