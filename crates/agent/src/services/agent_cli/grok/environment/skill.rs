//! Bounded native Skill manifests; directory names alone do not prove assets.
use crate::models::{
    AgentAssetCategory, AgentAssetDeclarationRole, AgentAssetDeclaredState, AgentAssetDetails,
    AgentAssetDiagnostic, AgentAssetDocumentFormat, AgentAssetPresence,
    AgentAssetResolutionParticipation, AgentAssetScope, AgentAssetSourceKind,
    AgentConfigurationContext, AgentSkillInvocationPolicy,
};
use crate::services::agent_cli::contracts::{
    AgentAssetNativePayload, AgentAssetParseRequest, AgentAssetSnapshot,
    AgentAssetSourcePathPolicy, AgentAssetSourceSpec, AgentFollowUpSourceDiscoveryRequest,
    AgentFollowUpSourceSpec, AgentFollowUpSourceTarget, AgentParseOutput, FollowUpSourceOutput,
    GrokStateControl, ParsedAgentAsset,
};
use crate::services::agent_cli::environment::{
    declaration_matches_source, parsed_asset, physical_origin, ParsedAssetInput,
};
use std::{collections::BTreeMap, path::PathBuf};

pub(super) const MANIFEST_PREFIX: &str = "grok-skill-manifest:";

#[cfg(test)]
mod tests;

pub(super) fn valid_declaration(
    context: &AgentConfigurationContext,
    asset: &ParsedAgentAsset,
    source: &AgentAssetSourceSpec,
) -> bool {
    if !declaration_matches_source(context, asset, source)
        || source.source_kind != AgentAssetSourceKind::File
        || asset.presence != AgentAssetPresence::Present
        || asset.logical_origin != physical_origin(source)
        || asset.participation != AgentAssetResolutionParticipation::Participates
        || asset.trust_state != context.trust_context
        || asset.provided_by.is_some()
        || asset.action_owner.is_some()
        || !asset.explicitly_affected.is_empty()
        || asset.resolution_group_key != asset.native_id
    {
        return false;
    }
    match asset.native_payload {
        AgentAssetNativePayload::GrokSkillDefinition => {
            asset.role == AgentAssetDeclarationRole::Definition
                && asset.declaration_key == "SKILL.md"
                && asset.declared_state == AgentAssetDeclaredState::Unknown
                && source
                    .native_source_key
                    .strip_prefix(MANIFEST_PREFIX)
                    .and_then(|value| value.split_once(':'))
                    .is_some_and(|(root, entry)| {
                        !entry.is_empty()
                            && matches!(
                                root,
                                "skills"
                                    | "shared-skills"
                                    | "workspace-skills"
                                    | "workspace-shared-skills"
                            )
                    })
                && source
                    .path
                    .file_name()
                    .is_some_and(|name| name == "SKILL.md")
                && normalized_name(&asset.native_id).as_deref() == Some(asset.native_id.as_str())
                && matches!(
                    asset.details,
                    AgentAssetDetails::Skill {
                        enabled: AgentAssetDeclaredState::Unknown,
                        invocation_policy: AgentSkillInvocationPolicy::ModelInvocable
                            | AgentSkillInvocationPolicy::ManualOnly,
                    }
                )
        }
        AgentAssetNativePayload::GrokStateControl(
            control @ (GrokStateControl::SkillDisabled | GrokStateControl::InvalidSkill),
        ) => {
            let state = if control == GrokStateControl::SkillDisabled {
                AgentAssetDeclaredState::Disabled
            } else {
                AgentAssetDeclaredState::Unknown
            };
            source.native_source_key == "config"
                && source.scope == AgentAssetScope::User
                && asset.role == AgentAssetDeclarationRole::StateOverlay
                && asset.declared_state == state
                && matches!(asset.details, AgentAssetDetails::Skill { enabled, invocation_policy: AgentSkillInvocationPolicy::Unknown } if enabled == state)
                && if control == GrokStateControl::SkillDisabled {
                    asset.declaration_key == format!("skills.disabled:{}", asset.native_id)
                } else {
                    asset.native_id == "__grok_invalid_control__"
                        && matches!(
                            asset.declaration_key.as_str(),
                            "invalid:skills" | "invalid:skills.disabled" | "invalid:source"
                        )
                }
        }
        _ => false,
    }
}

pub(in crate::services::agent_cli::grok) fn discover_follow_up_sources(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    let parent_key = request.parent.native_source_key.as_str();
    if !matches!(
        parent_key,
        "skills" | "shared-skills" | "workspace-skills" | "workspace-shared-skills"
    ) {
        return;
    }
    for entry in request.manifest {
        if entry.name.trim().is_empty() {
            continue;
        }
        let target = if entry.is_symlink {
            if entry.name == "SKILL.md"
                || !matches!(
                    request.parent.path_policy,
                    AgentAssetSourcePathPolicy::ReadonlySkillLinkRoot { .. }
                )
            {
                continue;
            }
            AgentFollowUpSourceTarget::ReadonlySkillDirectoryLink {
                directory_entry_name: entry.name.clone(),
            }
        } else {
            match entry.source_kind {
                AgentAssetSourceKind::Directory => AgentFollowUpSourceTarget::Descendant {
                    directory_entry_name: entry.name.clone(),
                    relative_path: PathBuf::from("SKILL.md"),
                },
                AgentAssetSourceKind::File if entry.name == "SKILL.md" => {
                    AgentFollowUpSourceTarget::ManifestFile {
                        entry_name: entry.name.clone(),
                    }
                }
                _ => continue,
            }
        };
        if output
            .emit_follow_up(AgentFollowUpSourceSpec {
                parent_source_key: parent_key.to_owned(),
                target,
                native_source_key: format!("{MANIFEST_PREFIX}{parent_key}:{}", entry.name),
                label: format!("Grok Build Skill：{}", entry.name),
                scope: request.parent.scope,
                precedence: request.parent.precedence,
                sensitive: false,
                source_kind: AgentAssetSourceKind::File,
                categories: vec![AgentAssetCategory::Skill],
            })
            .is_break()
        {
            return;
        }
    }
}

pub(super) fn parse_manifest(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    let AgentAssetSnapshot::File { bytes, .. } = request.snapshot else {
        return;
    };
    let parsed = std::str::from_utf8(bytes).ok().and_then(|text| {
        let directory_name = request.source.path.parent()?.file_name()?.to_str()?;
        decode_manifest(text, directory_name)
    });
    let Some((name, invocation_policy)) = parsed else {
        output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
            format: AgentAssetDocumentFormat::Manifest,
            location: None,
        });
        return;
    };
    let mut asset = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: "SKILL.md",
            resolution_group_key: &name,
            category: AgentAssetCategory::Skill,
            native_id: &name,
            label: &name,
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: request.context.trust_context,
            role: AgentAssetDeclarationRole::Definition,
            participation: AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Skill {
                enabled: AgentAssetDeclaredState::Unknown,
                invocation_policy,
            },
            facts: BTreeMap::new(),
        },
    );
    asset.native_payload = AgentAssetNativePayload::GrokSkillDefinition;
    let _ = output.emit_declaration(asset);
}

fn decode_manifest(
    text: &str,
    directory_name: &str,
) -> Option<(String, AgentSkillInvocationPolicy)> {
    let metadata = decode_metadata(text)?;
    let name = metadata
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(directory_name);
    Some((normalized_name(name)?, metadata.invocation_policy))
}

pub(super) struct SkillMetadata {
    pub name: Option<String>,
    pub invocation_policy: AgentSkillInvocationPolicy,
}

pub(super) fn decode_metadata(text: &str) -> Option<SkillMetadata> {
    let metadata = split_frontmatter(text)?;
    let field = |key: &str| metadata.get(serde_yaml_ng::Value::String(key.to_owned()));
    let name = match field("name") {
        Some(serde_yaml_ng::Value::String(name)) => Some(name.clone()),
        None => None,
        _ => return None,
    };
    // Native description/body are not needed for state or identity and do not
    // enter inventory facts. Their contents belong to the protected preview.
    if field("description").is_some_and(|value| !value.is_string()) {
        return None;
    }
    let invocation_policy = match field("disable-model-invocation") {
        Some(serde_yaml_ng::Value::Bool(true)) => AgentSkillInvocationPolicy::ManualOnly,
        Some(serde_yaml_ng::Value::Bool(false)) | None => {
            AgentSkillInvocationPolicy::ModelInvocable
        }
        _ => return None,
    };
    Some(SkillMetadata {
        name,
        invocation_policy,
    })
}

pub(super) fn normalized_name(name: &str) -> Option<String> {
    // The pinned native inspect fixtures establish ASCII slug normalization.
    // Do not assign an invented identity to unverified Unicode names.
    if !name.is_ascii() {
        return None;
    }
    let mut normalized = String::new();
    let mut separator = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !normalized.is_empty() {
                normalized.push('-');
            }
            normalized.push(character.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    (!normalized.is_empty() && normalized.len() <= 64).then_some(normalized)
}

fn split_frontmatter(text: &str) -> Option<serde_yaml_ng::Mapping> {
    let Some(body) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return Some(serde_yaml_ng::Mapping::new());
    };
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            let metadata = match serde_yaml_ng::from_str(&body[..offset]).ok()? {
                serde_yaml_ng::Value::Mapping(metadata) => metadata,
                serde_yaml_ng::Value::Null => serde_yaml_ng::Mapping::new(),
                _ => return None,
            };
            return Some(metadata);
        }
        offset += line.len();
    }
    None
}
