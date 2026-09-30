use super::*;
use serde_json::json;

fn location(handler: usize) -> HookLocation {
    HookLocation {
        event: "SessionStart".to_owned(),
        group: 0,
        handler,
    }
}

#[test]
fn deletion_rekeys_each_remaining_state_and_preserves_its_trust_and_unknown_fields() {
    let source = "/fixture/hooks.json";
    let key = |index| hooks::native_id(source, "SessionStart", 0, index);
    let mut policy = json!({"hooks":{"state":{
        (key(0)): {"enabled":false,"trusted_hash":"removed"},
        (format!(" {} ",key(1))): {"enabled":false,"trusted_hash":"sibling","custom":17},
        (key(2)): {"enabled":true,"trusted_hash":"last"},
        "/unrelated:session_start:0:0": {"trusted_hash":"retain"}
    }},"model":"fixture"});
    let final_document =
        json!({"hooks":{"SessionStart":[{"hooks":[{"command":"second"},{"command":"third"}]}]}});
    let locations = BTreeMap::from([
        (location(0), None),
        (location(1), Some(location(0))),
        (location(2), Some(location(1))),
    ]);
    rekey_states(&mut policy, source, &final_document, &locations).unwrap();
    let states = &policy["hooks"]["state"];
    assert_eq!(
        states[format!(" {} ", key(0))],
        json!({"enabled":false,"trusted_hash":"sibling","custom":17})
    );
    assert_eq!(
        states[key(1)],
        json!({"enabled":true,"trusted_hash":"last"})
    );
    assert!(states.get(key(2)).is_none());
    assert!(states.get(key(0)).is_none());
    assert_eq!(
        states["/unrelated:session_start:0:0"]["trusted_hash"],
        "retain"
    );
    assert_eq!(policy["model"], "fixture");
}

#[test]
fn adding_a_new_occupant_drops_stale_trust_without_authorizing_the_new_rule() {
    let source = "/fixture/hooks.json";
    let stale = hooks::native_id(source, "SessionStart", 0, 0);
    let mut policy =
        json!({"hooks":{"state":{(stale.clone()):{"enabled":true,"trusted_hash":"old occupant"}}}});
    let final_document = json!({"hooks":{"SessionStart":[{"hooks":[{"command":"new occupant"}]}]}});
    rekey_states(&mut policy, source, &final_document, &BTreeMap::new()).unwrap();
    assert!(hooks::decode_states(&policy).is_empty());
    assert!(policy["hooks"]["state"].get(stale).is_none());
}

#[test]
fn explicit_switch_removes_competing_trimmed_aliases_but_never_changes_trusted_hash() {
    let key = "/fixture/hooks.json:session_start:0:0";
    let mut policy = json!({"hooks":{"state":{
        (format!(" {key}")): {"enabled":true,"trusted_hash":"before"},
        (format!("{key} ")): {"enabled":true,"custom":"retain"}
    }}});
    set_enabled(&mut policy, key, false).unwrap();
    let states = hooks::decode_states(&policy);
    assert_eq!(states[key].enabled, Some(false));
    assert_eq!(states[key].trusted_hash.as_deref(), Some("before"));
    assert_eq!(
        policy["hooks"]["state"][format!("{key} ")]["custom"],
        "retain"
    );
    set_enabled(&mut policy, key, true).unwrap();
    assert_eq!(
        hooks::decode_states(&policy)[key].trusted_hash.as_deref(),
        Some("before")
    );
}

#[test]
fn native_schema_rejects_unloaded_or_unrepresentable_handlers() {
    for handler in [
        json!({"type":"prompt","prompt":"fixture"}),
        json!({"type":"command","command":"  "}),
        json!({"type":"command","command":"x","timeout":-1}),
        json!({"type":"mcp_tool","server":"s","tool":"t","input":{"nested":null}}),
    ] {
        let definition = HookNativeDefinition {
            event: "SessionStart".to_owned(),
            group: json!({"hooks":[handler]}),
        };
        assert!(validate_definition(&definition).is_err());
    }
    let definition = HookNativeDefinition {
        event: "SessionEnd".to_owned(),
        group: json!({"hooks":[{"type":"mcp_tool","server":"s","tool":"t"}]}),
    };
    assert!(validate_definition(&definition).is_err());
}
