use super::super::super::environment::{parsed_asset, physical_origin, ParsedAssetInput};
use super::parse::extension_ref;
use super::{emit_malformed, participation};
use crate::models::{
    AgentAssetCategory, AgentAssetDeclaredState, AgentAssetDetails, AgentSkillInvocationPolicy,
};
use crate::services::agent_cli::contracts::{
    AgentAssetNativePayload, AgentAssetParseRequest, AgentOutputStop, AgentParseOutput,
    AgentSkillPolicyPayload,
};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, ops::ControlFlow};

/// Parse Gemini's bounded SKILL.md frontmatter. Gemini first treats the
/// frontmatter as YAML and falls back to its compatibility line parser when
/// YAML is malformed or the required fields are not YAML strings.
pub(super) fn parse_skill(request: AgentAssetParseRequest<'_>, output: &mut dyn AgentParseOutput) {
    let crate::services::agent_cli::contracts::AgentAssetSnapshot::File { bytes, .. } =
        request.snapshot
    else {
        return;
    };
    let Ok(text) = std::str::from_utf8(bytes) else {
        emit_malformed(output, "SKILL.md");
        return;
    };
    let Some(frontmatter) = extract_frontmatter(text) else {
        emit_malformed(output, "SKILL.md");
        return;
    };
    let fields = parse_yaml_fields(frontmatter).or_else(|| parse_simple_fields(frontmatter));
    let Some((name, _description)) = fields else {
        emit_malformed(output, "SKILL.md");
        return;
    };
    let source_key = &request.source.native_source_key;
    let native_id = sanitize_skill_name(&name);
    let parent = source_key
        .strip_prefix(super::EXTENSION_SKILL_PREFIX)
        .and_then(|value| {
            value
                .split_once(':')
                .map(|(extension, _)| extension_ref(extension))
        });
    let declaration = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: source_key,
            resolution_group_key: &native_id,
            category: AgentAssetCategory::Skill,
            native_id: &native_id,
            label: &native_id,
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: request.context.trust_context,
            role: crate::models::AgentAssetDeclarationRole::Definition,
            participation: participation(request, AgentAssetCategory::Skill),
            provided_by: parent.clone(),
            action_owner: parent,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Skill {
                enabled: AgentAssetDeclaredState::Unknown,
                invocation_policy: AgentSkillInvocationPolicy::Unknown,
            },
            facts: BTreeMap::new(),
        },
    );
    let _: ControlFlow<AgentOutputStop> = output.emit_declaration(declaration);
}

pub(super) fn parse_disabled_policy(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let Some(skills) = root.get("skills") else {
        return ControlFlow::Continue(());
    };
    let Some(skills) = skills.as_object() else {
        emit_malformed(output, "skills");
        return emit_invalid_policy(request, output);
    };
    let Some(disabled) = skills.get("disabled") else {
        return ControlFlow::Continue(());
    };
    let Some(names) = disabled.as_array().and_then(|values| {
        values
            .iter()
            .map(|value| value.as_str().map(str::to_owned))
            .collect::<Option<_>>()
    }) else {
        emit_malformed(output, "skills.disabled");
        return emit_invalid_policy(request, output);
    };
    emit_policy(request, AgentSkillPolicyPayload::DisabledSet(names), output)
}

pub(super) fn emit_invalid_policy(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    emit_policy(request, AgentSkillPolicyPayload::InvalidDisabledSet, output)
}

fn emit_policy(
    request: AgentAssetParseRequest<'_>,
    payload: AgentSkillPolicyPayload,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let mut declaration = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: if matches!(payload, AgentSkillPolicyPayload::InvalidDisabledSet) {
                "skills.disabled.invalid"
            } else {
                "skills.disabled"
            },
            resolution_group_key: "skills.disabled",
            category: AgentAssetCategory::Skill,
            native_id: "skills.disabled",
            label: "Gemini CLI Skill 禁用策略",
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: request.context.trust_context,
            role: crate::models::AgentAssetDeclarationRole::PolicyOverlay,
            participation: participation(request, AgentAssetCategory::Skill),
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Skill {
                enabled: AgentAssetDeclaredState::Unknown,
                invocation_policy: AgentSkillInvocationPolicy::Unknown,
            },
            facts: BTreeMap::new(),
        },
    );
    declaration.native_payload = AgentAssetNativePayload::SkillPolicy(payload);
    output.emit_declaration(declaration)
}

fn extract_frontmatter(text: &str) -> Option<&str> {
    // Do not trim a BOM: Gemini's anchored frontmatter matcher rejects it.
    let body = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let line_start = offset;
        let line_end = line.len();
        let without_newline = line.strip_suffix('\n').unwrap_or(line);
        let value = without_newline
            .strip_suffix('\r')
            .unwrap_or(without_newline);
        if value == "---" {
            return Some(&body[..line_start]);
        }
        offset += line_end;
    }
    None
}

fn parse_yaml_fields(frontmatter: &str) -> Option<(String, String)> {
    let value = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(frontmatter).ok()?;
    let serde_yaml_ng::Value::Mapping(mapping) = value else {
        return None;
    };
    let key = |name: &str| {
        mapping
            .get(serde_yaml_ng::Value::String(name.to_owned()))
            .and_then(|value| match value {
                serde_yaml_ng::Value::String(value) => Some(value.clone()),
                _ => None,
            })
    };
    Some((key("name")?, key("description")?))
}

fn parse_simple_fields(frontmatter: &str) -> Option<(String, String)> {
    let mut name = None;
    let mut description = None;
    let lines = frontmatter.lines().collect::<Vec<_>>();
    let mut index = 0;
    while index < lines.len() {
        let raw_line = lines[index];
        let trimmed = raw_line.trim_start();
        let Some((key, value)) = trimmed.split_once(':') else {
            index += 1;
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        let mut next_index = index + 1;
        match key {
            "name" => name = Some(value.to_owned()),
            "description" => {
                description = Some(value.to_owned());
                while next_index < lines.len() {
                    let continuation = lines[next_index];
                    let continuation_value = continuation.trim_start();
                    if !(continuation.starts_with(' ') || continuation.starts_with('\t'))
                        || continuation_value.is_empty()
                    {
                        break;
                    }
                    let description = description.as_mut().expect("description was set");
                    if !description.is_empty() {
                        description.push(' ');
                    }
                    description.push_str(continuation_value);
                    next_index += 1;
                }
            }
            _ => {}
        }
        index = next_index;
    }
    Some((name?.trim().to_owned(), description?.trim().to_owned()))
}

pub(super) fn sanitize_skill_name(name: &str) -> String {
    name.chars()
        .map(|value| match value {
            ':' | '\\' | '/' | '<' | '>' | '*' | '?' | '"' | '|' => '-',
            value => value,
        })
        .collect()
}
