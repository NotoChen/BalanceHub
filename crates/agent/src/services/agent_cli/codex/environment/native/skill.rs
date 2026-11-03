use super::{malformed, sources, PLUGIN_SKILLS_PREFIX, SKILL_PREFIX};
use crate::models::{
    AgentAssetCategory, AgentAssetDeclarationRole, AgentAssetDeclaredState, AgentAssetDetails,
    AgentAssetDiagnostic, AgentAssetDocumentFormat, AgentAssetNativeRef, AgentAssetPresence,
    AgentAssetResolutionParticipation, AgentAssetScope, AgentAssetSourceKind,
    AgentAssetSuppressionReason, AgentSkillInvocationPolicy,
};
use crate::services::agent_cli::contracts::{
    AgentAssetNativePayload, AgentAssetParseRequest, AgentAssetSnapshot, AgentAssetSourceSpec,
    AgentDefinitionSelectionRequest, AgentDefinitionSuppression, AgentOutputStop, AgentParseOutput,
    CodexAssetPayload, CodexSkillRule, CodexSkillSelector,
};
use crate::services::agent_cli::environment::{
    declaration_matches_source, declaration_trust_for_participation, parsed_asset, physical_origin,
    ParsedAssetInput,
};
use crate::services::agent_cli::native_agent_kinds::codex::AGENT_KIND;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::ControlFlow,
    path::Path,
};

pub(in crate::services::agent_cli::codex::environment) const RULES_KEY: &str = "skills.config";
pub(in crate::services::agent_cli::codex::environment) const RULES_ID: &str =
    "__balancehub_codex_skill_rules__";

#[derive(Deserialize)]
struct SkillMetadata {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    metadata: ShortMetadata,
}

#[derive(Default, Deserialize)]
struct ShortMetadata {
    #[serde(default, rename = "short-description")]
    short_description: Option<String>,
}

pub(super) fn validate_skill(text: &str, directory_name: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut frontmatter = Vec::new();
    let mut closed = false;
    for line in lines {
        if line.trim() == "---" {
            closed = true;
            break;
        }
        frontmatter.push(line);
    }
    if !closed || frontmatter.is_empty() {
        return None;
    }
    let frontmatter = frontmatter.join("\n");
    let metadata = serde_yaml_ng::from_str::<SkillMetadata>(&frontmatter)
        .ok()
        .or_else(|| serde_yaml_ng::from_str(&repair_scalars(&frontmatter)?).ok())?;
    let name = metadata
        .name
        .as_deref()
        .map(single_line)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| single_line(directory_name));
    let description = metadata
        .description
        .as_deref()
        .map(single_line)
        .unwrap_or_default();
    // Decode optional metadata with its native type without retaining document prose.
    let _ = metadata.metadata.short_description;
    (!name.is_empty() && name.chars().count() <= 64 && !description.is_empty()).then_some(name)
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

// This is the pinned native line-oriented repair. It does not turn arbitrary
// malformed YAML into an asset or repair mapping/sequence structure.
fn repair_scalars(frontmatter: &str) -> Option<String> {
    let mut changed = false;
    let mut block_indent = None;
    let mut repaired = Vec::new();
    for line in frontmatter.lines() {
        let indent = line
            .chars()
            .take_while(|character| *character == ' ')
            .count();
        if let Some(block) = block_indent {
            if line.trim().is_empty() || indent > block {
                repaired.push(line.to_owned());
                continue;
            }
            block_indent = None;
        }
        let Some((key, value)) = line.split_once(':') else {
            repaired.push(line.to_owned());
            continue;
        };
        if key.trim().is_empty()
            || value
                .chars()
                .next()
                .is_some_and(|value| !value.is_whitespace())
        {
            repaired.push(line.to_owned());
            continue;
        }
        let start = value.trim_start();
        let whitespace = &value[..value.len() - start.len()];
        let mut scalar = start;
        let mut comment = "";
        for (offset, character) in start.char_indices() {
            if character == '#'
                && (offset == 0
                    || start[..offset]
                        .chars()
                        .next_back()
                        .is_some_and(char::is_whitespace))
            {
                let end = start[..offset].trim_end().len();
                scalar = &start[..end];
                comment = &start[end..];
                break;
            }
        }
        let scalar = scalar.trim_end();
        let Some(first) = scalar.chars().next() else {
            repaired.push(line.to_owned());
            continue;
        };
        if matches!(first, '|' | '>') {
            block_indent = Some(indent);
            repaired.push(line.to_owned());
            continue;
        }
        if matches!(first, '\'' | '"') {
            repaired.push(line.to_owned());
            continue;
        }
        let colon_separator = scalar.char_indices().any(|(offset, character)| {
            character == ':'
                && scalar[offset + 1..]
                    .chars()
                    .next()
                    .is_some_and(char::is_whitespace)
        });
        let invalid_flow = matches!(first, '[' | '{' | '@' | '`')
            && serde_yaml_ng::from_str::<serde_yaml_ng::Value>(scalar).is_err();
        if !colon_separator && !invalid_flow {
            repaired.push(line.to_owned());
            continue;
        }
        repaired.push(format!(
            "{key}:{whitespace}'{}{comment}",
            scalar.replace('\'', "''") + "'"
        ));
        changed = true;
    }
    changed.then(|| repaired.join("\n"))
}

pub(super) fn parse_manifest(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    let AgentAssetSnapshot::File { bytes, .. } = request.snapshot else {
        return;
    };
    let Some(canonical_path) = request
        .source
        .definition_identity_path()
        .map(Path::to_path_buf)
    else {
        output.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection {
            projection_key: request.source.native_source_key.clone(),
        });
        return;
    };
    let Some(raw_name) = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| validate_skill(text, canonical_path.parent()?.file_name()?.to_str()?))
    else {
        malformed(output, AgentAssetDocumentFormat::Manifest);
        return;
    };
    let root_key = request
        .source
        .native_source_key
        .strip_prefix(SKILL_PREFIX)
        .and_then(|key| key.rsplit_once(':').map(|(root, _)| root));
    let plugin = root_key
        .and_then(|key| key.strip_prefix(PLUGIN_SKILLS_PREFIX))
        .and_then(sources::plugin_skill_source_parts);
    let (name, parent, plugin_id) = match plugin {
        Some((plugin_id, _, namespace, _, _)) => {
            let name = format!("{namespace}:{raw_name}");
            let reference = AgentAssetNativeRef {
                category: AgentAssetCategory::Plugin,
                native_id: plugin_id.to_owned(),
                qualifier: Some(format!("plugin:{plugin_id}")),
            };
            (name, Some(reference), Some(plugin_id.to_owned()))
        }
        None => (raw_name, None, None),
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
            trust_state: declaration_trust_for_participation(
                request.context.trust_context,
                AgentAssetResolutionParticipation::Participates,
            ),
            role: AgentAssetDeclarationRole::Definition,
            participation: AgentAssetResolutionParticipation::Participates,
            provided_by: parent,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Skill {
                enabled: AgentAssetDeclaredState::Unknown,
                invocation_policy: AgentSkillInvocationPolicy::Unknown,
            },
            facts: BTreeMap::new(),
        },
    );
    asset.native_payload =
        AgentAssetNativePayload::CodexAsset(CodexAssetPayload::SkillDefinition {
            canonical_path,
            plugin_id,
        });
    let _ = output.emit_declaration(asset);
}

pub(in crate::services::agent_cli::codex) fn definition_suppressions(
    request: AgentDefinitionSelectionRequest<'_>,
) -> Vec<AgentDefinitionSuppression> {
    if request.context.agent_kind != AGENT_KIND {
        return Vec::new();
    }
    let sources = request
        .sources
        .iter()
        .map(|source| (source.native_source_key.as_str(), source))
        .collect::<BTreeMap<_, _>>();
    if sources.len() != request.sources.len()
        || request
            .declarations
            .iter()
            .map(|asset| &asset.declaration_id)
            .collect::<BTreeSet<_>>()
            .len()
            != request.declarations.len()
    {
        return Vec::new();
    }
    let mut candidates = request
        .declarations
        .iter()
        .enumerate()
        .filter_map(|(order, asset)| {
            let (root_key, entry) = sources::skill_source_parts(&asset.source_key)?;
            let root = *sources.get(root_key)?;
            let root_order = standalone_root_order(root)?;
            let source = *sources.get(asset.source_key.as_str())?;
            let AgentAssetNativePayload::CodexAsset(CodexAssetPayload::SkillDefinition {
                canonical_path,
                plugin_id: None,
            }) = &asset.native_payload
            else {
                return None;
            };
            let expected = if entry == "SKILL.md" {
                root.path.join("SKILL.md")
            } else {
                root.path.join(entry).join("SKILL.md")
            };
            if asset.category != AgentAssetCategory::Skill
                || asset.role != AgentAssetDeclarationRole::Definition
                || asset.participation != AgentAssetResolutionParticipation::Participates
                || asset.presence != AgentAssetPresence::Present
                || asset.declaration_key != "SKILL.md"
                || asset.declared_state != AgentAssetDeclaredState::Unknown
                || asset.native_id != asset.resolution_group_key
                || asset.provided_by.is_some()
                || asset.action_owner.is_some()
                || !asset.explicitly_affected.is_empty()
                || !declaration_matches_source(request.context, asset, source)
                || asset.logical_origin != physical_origin(source)
                || root.source_kind != AgentAssetSourceKind::Directory
                || !root.categories.contains(&AgentAssetCategory::Skill)
                || source.source_kind != AgentAssetSourceKind::File
                || source.scope != root.scope
                || source.precedence != root.precedence
                || source.path != expected
                || source.verified_physical_path.is_none()
                || source.definition_identity_path() != Some(canonical_path.as_path())
                || !canonical_path.is_absolute()
                || canonical_path
                    .file_name()
                    .is_none_or(|name| name != "SKILL.md")
            {
                return None;
            }
            Some((
                root_order,
                order,
                &asset.declaration_id,
                canonical_path.as_path(),
            ))
        })
        .collect::<Vec<_>>();
    // Native root order precedes canonical deduplication. Within one root keep
    // the existing deterministic discovery order; display precedence is separate.
    candidates.sort_by_key(|(root_order, order, _, _)| (*root_order, *order));
    let mut seen = BTreeSet::new();
    let mut duplicates = Vec::new();
    for (_, _, id, canonical) in candidates {
        if !seen.insert(canonical) {
            duplicates.push(AgentDefinitionSuppression {
                declaration_id: id.to_owned(),
                reason: AgentAssetSuppressionReason::DuplicatePhysicalSource,
            });
        }
    }
    duplicates
}

fn standalone_root_order(root: &AgentAssetSourceSpec) -> Option<u8> {
    match (root.native_source_key.as_str(), root.scope) {
        ("workspace-codex-skills", AgentAssetScope::Workspace) => Some(0),
        ("codex-skills", AgentAssetScope::User) => Some(1),
        ("shared-skills", AgentAssetScope::User) => Some(2),
        ("system-skills", AgentAssetScope::System) => Some(3),
        ("admin-skills", AgentAssetScope::Managed) => Some(4),
        ("workspace-skills", AgentAssetScope::Workspace) => Some(5),
        _ => None,
    }
}

#[derive(Deserialize)]
struct NativeRules {
    #[serde(default)]
    config: Vec<NativeRule>,
    #[serde(default)]
    bundled: Option<Bundled>,
    #[serde(default)]
    include_instructions: Option<bool>,
    #[serde(default)]
    max_context_tokens: Option<std::num::NonZeroUsize>,
}
#[derive(Deserialize)]
struct Bundled {
    #[serde(default = "enabled_default")]
    enabled: bool,
}
fn enabled_default() -> bool {
    true
}
#[derive(Deserialize)]
struct NativeRule {
    path: Option<std::path::PathBuf>,
    name: Option<String>,
    enabled: bool,
}

pub(super) fn skill_rules(root: &toml::value::Table) -> Result<Vec<CodexSkillRule>, ()> {
    let Some(value) = root.get("skills") else {
        return Ok(Vec::new());
    };
    let config: NativeRules = value.clone().try_into().map_err(|_| ())?;
    let _ = (
        config.bundled.map(|value| value.enabled),
        config.include_instructions,
        config.max_context_tokens,
    );
    let mut rules: Vec<CodexSkillRule> = Vec::new();
    for rule in config.config {
        let selector = match (rule.path, rule.name) {
            (Some(path), None) if path.is_absolute() => {
                CodexSkillSelector::Path(path.canonicalize().unwrap_or(path))
            }
            (Some(_), None) => return Err(()),
            (None, Some(name)) if !name.trim().is_empty() => {
                CodexSkillSelector::Name(name.trim().to_owned())
            }
            _ => continue,
        };
        rules.retain(|previous| previous.selector != selector);
        rules.push(CodexSkillRule {
            selector,
            enabled: rule.enabled,
        });
    }
    Ok(rules)
}

pub(super) fn parse_rules(
    request: AgentAssetParseRequest<'_>,
    root: Option<&toml::value::Table>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let available =
        root.is_some() || matches!(request.snapshot, AgentAssetSnapshot::Missing { .. });
    let rules = match root.map(skill_rules).transpose() {
        Ok(rules) => rules.unwrap_or_default(),
        Err(()) => {
            malformed(output, AgentAssetDocumentFormat::Toml);
            Vec::new()
        }
    };
    let mut declaration = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: RULES_KEY,
            resolution_group_key: RULES_ID,
            category: AgentAssetCategory::Skill,
            native_id: RULES_ID,
            label: "Codex Skill 开关规则",
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: declaration_trust_for_participation(
                request.context.trust_context,
                AgentAssetResolutionParticipation::Participates,
            ),
            role: AgentAssetDeclarationRole::PolicyOverlay,
            participation: AgentAssetResolutionParticipation::Participates,
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
    declaration.native_payload =
        AgentAssetNativePayload::CodexAsset(CodexAssetPayload::SkillRules { rules, available });
    output.emit_declaration(declaration)
}
