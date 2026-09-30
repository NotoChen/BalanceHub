use super::super::parse::skill::{
    decode_skill_frontmatter, skill_frontmatter, SkillFrontmatterError,
};
use crate::models::{
    AgentAssetDiagnostic, AgentAssetDocumentFormat, AgentAssetLimitKind, AgentSkillInvocationPolicy,
};
use crate::services::agent_cli::contracts::{AgentDiagnosticEmission, AgentDiagnosticOutput};

#[derive(Default)]
struct Diagnostics(Vec<AgentAssetDiagnostic>);

impl AgentDiagnosticOutput for Diagnostics {
    fn has_regular_capacity(&self) -> bool {
        true
    }

    fn emit_diagnostic(&mut self, value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        self.0.push(value);
        AgentDiagnosticEmission::Accepted
    }
}

#[test]
fn skill_frontmatter_accepts_yaml_punctuation_comments_and_extensions() {
    for (text, expected_name) in [
        (
            "---\nname: review\ndescription: Review a change, then discuss the result.\n---\nbody",
            Some("review"),
        ),
        (
            "---\nname: 'owner''s skill'\ndescription: \"Review: one, two\"\n---",
            Some("owner's skill"),
        ),
        (
            "---\nname: \"review\\u003asecurity\" # native name\n# comment\n---",
            Some("review:security"),
        ),
        (
            "---\nname: review # native name\ndescription: >-\n  Review a change,\n  then discuss the result.\nmetadata:\n  labels: [review, security]\n  nested:\n    optional: null\n    enabled: true\n---",
            Some("review"),
        ),
        (
            "---\nname: review\ndescription: |\n  Text before a divider.\n  ---\n  Text after a divider.\n---\nbody",
            Some("review"),
        ),
        ("---\n-name: value\n?name: value\n---", None),
        ("---\n# Only commentary\n---", None),
        ("---\n---", None),
        ("---\n{}\n---", None),
        ("Body without frontmatter.", None),
    ] {
        let metadata = skill_frontmatter(text).unwrap_or_else(|error| panic!("{error:?}: {text}"));
        assert_eq!(metadata.name.as_deref(), expected_name);
        assert_eq!(metadata.invocation_policy, AgentSkillInvocationPolicy::ModelInvocable);
    }
}

#[test]
fn skill_frontmatter_preserves_native_invocation_policy_with_yaml_merges() {
    for text in [
        "---\nname: review\ndisable-model-invocation: true\n---",
        "---\ndefaults: &defaults\n  disable-model-invocation: true\n<<: *defaults\nname: review\n---",
        "---\nname: review\ndisable-model-invocation: true # manual only\n---",
    ] {
        let metadata = skill_frontmatter(text).unwrap();
        assert_eq!(metadata.name.as_deref(), Some("review"));
        assert_eq!(metadata.invocation_policy, AgentSkillInvocationPolicy::ManualOnly);
    }
    let explicit_override = skill_frontmatter(
        "---\ndefaults: &defaults\n  disable-model-invocation: true\n<<: *defaults\ndisable-model-invocation: false\n---",
    ).unwrap();
    assert_eq!(
        explicit_override.invocation_policy,
        AgentSkillInvocationPolicy::ModelInvocable
    );
}

#[test]
fn skill_frontmatter_resolves_transitive_merges_before_reading_policy() {
    let inherited = "---\nbase: &base\n  disable-model-invocation: true\ndefaults: &defaults\n  <<: *base\n<<: *defaults\nname: review\n---";
    let metadata = skill_frontmatter(inherited).unwrap();
    assert_eq!(metadata.name.as_deref(), Some("review"));
    assert_eq!(
        metadata.invocation_policy,
        AgentSkillInvocationPolicy::ManualOnly
    );

    let overridden = inherited.replace(
        "name: review",
        "name: review\ndisable-model-invocation: false",
    );
    let metadata = skill_frontmatter(&overridden).unwrap();
    assert_eq!(metadata.name.as_deref(), Some("review"));
    assert_eq!(
        metadata.invocation_policy,
        AgentSkillInvocationPolicy::ModelInvocable
    );

    let sequence = "---\nbase: &base {disable-model-invocation: true}\nfirst: &first {<<: *base}\nsecond: &second {disable-model-invocation: false}\n<<: [*first, *second]\n---";
    assert_eq!(
        skill_frontmatter(sequence).unwrap().invocation_policy,
        AgentSkillInvocationPolicy::ManualOnly
    );
}

#[test]
fn skill_frontmatter_rejects_nested_invalid_merges() {
    for text in [
        "---\n<<:\n  <<: invalid-merge-value\n---",
        "---\n<<:\n  <<: [invalid-merge-value]\ndisable-model-invocation: false\n---",
    ] {
        assert_eq!(
            skill_frontmatter(text),
            Err(SkillFrontmatterError::Syntax),
            "{text}"
        );
        let mut diagnostics = Diagnostics::default();
        assert!(decode_skill_frontmatter(text.as_bytes(), &mut diagnostics).is_none());
        assert_eq!(
            diagnostics.0,
            [AgentAssetDiagnostic::Malformed {
                format: AgentAssetDocumentFormat::Yaml,
                location: Some("frontmatter.syntax".to_owned()),
            }]
        );
    }
}

#[test]
fn skill_frontmatter_reports_parser_resource_limits_as_complexity() {
    let deep = format!(
        "---\nmetadata: {}0{}\n---",
        "[".repeat(140),
        "]".repeat(140)
    );
    assert!(deep.len() < 300);
    let mut repeated = String::from("---\na: &a [zero, one]\n");
    for (name, previous) in [("b", "a"), ("c", "b"), ("d", "c"), ("e", "d")] {
        repeated.push_str(&format!(
            "{name}: &{name} [{}]\n",
            vec![format!("*{previous}"); 8].join(",")
        ));
    }
    repeated.push_str("metadata: [*e,*e,*e,*e,*e,*e,*e,*e]\n---");
    for text in [
        deep.as_str(),
        "---\ncycle: &cycle\n  <<: *cycle\n<<: *cycle\n---",
        repeated.as_str(),
    ] {
        assert!(text.len() < 16 * 1024);
        assert!(text.lines().count() < 64);
        assert_eq!(
            skill_frontmatter(text),
            Err(SkillFrontmatterError::Complexity)
        );
        let mut diagnostics = Diagnostics::default();
        assert!(decode_skill_frontmatter(text.as_bytes(), &mut diagnostics).is_none());
        assert_eq!(
            diagnostics.0,
            [AgentAssetDiagnostic::Malformed {
                format: AgentAssetDocumentFormat::Yaml,
                location: Some("frontmatter.complexity".to_owned()),
            }]
        );
    }
}

#[test]
fn skill_frontmatter_bounds_expanded_yaml_before_merge_normalization() {
    let text = format!(
        "---\nitem: &entry [a,b,c,d,e,f,g,h]\nmetadata: [{}]\n---",
        vec!["*entry"; 2000].join(","),
    );
    assert!(text.len() < 16 * 1024);
    assert!(text.lines().count() < 64);
    assert_eq!(
        skill_frontmatter(&text),
        Err(SkillFrontmatterError::Complexity)
    );
    let mut diagnostics = Diagnostics::default();
    assert!(decode_skill_frontmatter(text.as_bytes(), &mut diagnostics).is_none());
    assert_eq!(
        diagnostics.0,
        [AgentAssetDiagnostic::Malformed {
            format: AgentAssetDocumentFormat::Yaml,
            location: Some("frontmatter.complexity".to_owned()),
        }]
    );
}

#[test]
fn skill_frontmatter_rejects_invalid_yaml_and_duplicate_keys() {
    for text in [
        "---\nname: one\nname: two\n---",
        "---\ndisable-model-invocation: false\ndisable-model-invocation: true\n---",
        "---\nmetadata:\n  same: one\n  same: two\n---",
        "---\nname: *missing\n---",
        "---\nname: 'unterminated\n---",
        "---\nname: 'illegal' interior'\n---",
        "---\nname: \"dangling\\\"\n---",
        "---\nname: \"unknown\\x escape\"\n---",
        "---\nname: nested: value\n---",
        "---\nname: - sequence\n---",
        "---\nname: ? explicit\n---",
        "---\nname: %directive\n---",
        "---\nname: @reserved\n---",
        "---\nname: `tagged`\n---",
        "---\n%name: value\n---",
        "---\n@name: value\n---",
        "---\n`name: value\n---",
        "---\n<<: invalid-merge-value\n---",
    ] {
        assert_eq!(
            skill_frontmatter(text),
            Err(SkillFrontmatterError::Syntax),
            "{text}"
        );
    }
}

#[test]
fn skill_frontmatter_rejects_wrong_root_name_and_policy_types() {
    for text in [
        "null",
        "~",
        "true",
        "a string",
        "[one, two]",
        "- one\n- two",
    ] {
        assert_eq!(
            skill_frontmatter(&format!("---\n{text}\n---")),
            Err(SkillFrontmatterError::Root),
            "{text}"
        );
    }
    for value in [
        "",
        "null",
        "true",
        "42",
        "[]",
        "[one, two]",
        "{nested: true}",
        "\n  nested: true",
        "''",
        "' '",
        "|",
        "\"line\\nfeed\"",
        "\"carriage\\rreturn\"",
        "\"tab\\tvalue\"",
    ] {
        assert_eq!(
            skill_frontmatter(&format!("---\nname: {value}\n---")),
            Err(SkillFrontmatterError::Name),
            "{value}"
        );
    }
    for value in [
        "",
        "null",
        "\"true\"",
        "\"false\"",
        "1",
        "[]",
        "{enabled: true}",
    ] {
        assert_eq!(
            skill_frontmatter(&format!("---\ndisable-model-invocation: {value}\n---")),
            Err(SkillFrontmatterError::InvocationPolicy),
            "{value}"
        );
    }
}

#[test]
fn skill_frontmatter_has_exact_header_limits_without_limiting_the_body() {
    let line_limit = format!("---\n{}---\nbody", "# metadata\n".repeat(63));
    assert!(skill_frontmatter(&line_limit).is_ok());
    let too_many_lines = format!("---\n{}---\nbody", "# metadata\n".repeat(64));
    assert_eq!(
        skill_frontmatter(&too_many_lines),
        Err(SkillFrontmatterError::Lines {
            observed_at_least: 65
        })
    );

    for newline in ["\n", "\r\n"] {
        let empty_header = format!("---{newline}description: {newline}---{newline}");
        let padding = 16 * 1024 - empty_header.len();
        let header = format!(
            "---{newline}description: {}{newline}---{newline}",
            "x".repeat(padding)
        );
        assert_eq!(header.len(), 16 * 1024);
        assert!(skill_frontmatter(&header).is_ok());
        let large_body = format!("{header}{}", "body\n".repeat(16 * 1024));
        assert!(skill_frontmatter(&large_body).is_ok());
        let oversized = header.replacen("description: ", "description: x", 1);
        assert_eq!(
            skill_frontmatter(&oversized),
            Err(SkillFrontmatterError::Bytes {
                observed_at_least: 16 * 1024 + 1
            })
        );
    }
    for text in [
        "---",
        "---\n",
        "---\nname: review",
        "---\nname: review\n  ---",
    ] {
        assert_eq!(
            skill_frontmatter(text),
            Err(SkillFrontmatterError::Unterminated),
            "{text}"
        );
    }
}

#[test]
fn skill_frontmatter_diagnostics_distinguish_encoding_syntax_fields_and_limits() {
    for (bytes, location) in [
        (vec![0xff, 0xfe], "frontmatter.encoding"),
        (b"---\nname: review".to_vec(), "frontmatter.unterminated"),
        (
            b"---\nname: [unfinished\n---".to_vec(),
            "frontmatter.syntax",
        ),
        (b"---\n[]\n---".to_vec(), "frontmatter.root"),
        (b"---\nname: null\n---".to_vec(), "frontmatter.name"),
        (
            b"---\ndisable-model-invocation: 'false'\n---".to_vec(),
            "frontmatter.disable-model-invocation",
        ),
    ] {
        let mut diagnostics = Diagnostics::default();
        assert!(decode_skill_frontmatter(&bytes, &mut diagnostics).is_none());
        assert_eq!(
            diagnostics.0,
            [AgentAssetDiagnostic::Malformed {
                format: AgentAssetDocumentFormat::Yaml,
                location: Some(location.to_owned()),
            }]
        );
    }
    for (text, limit, accepted) in [
        (
            format!("---\n{}---", "# comment\n".repeat(64)),
            AgentAssetLimitKind::FrontmatterLines,
            64,
        ),
        (
            format!("---\ndescription: {}\n---", "x".repeat(16 * 1024)),
            AgentAssetLimitKind::FrontmatterBytes,
            16 * 1024,
        ),
    ] {
        let mut diagnostics = Diagnostics::default();
        assert!(decode_skill_frontmatter(text.as_bytes(), &mut diagnostics).is_none());
        assert!(
            matches!(diagnostics.0.as_slice(), [AgentAssetDiagnostic::Truncated {
            limit: actual_limit, accepted: actual_accepted, observed_at_least,
        }] if *actual_limit == limit && *actual_accepted == accepted && *observed_at_least > accepted)
        );
    }
    let mut diagnostics = Diagnostics::default();
    assert!(decode_skill_frontmatter(
        b"---\nname: review\ndescription: Review a change, then discuss.\n---",
        &mut diagnostics
    )
    .is_some());
    assert!(diagnostics.0.is_empty());
}
