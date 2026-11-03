use super::*;
use crate::models::{AgentAssetLimitKind, AgentSkillInvocationPolicy};
use serde_yaml_ng::Value as YamlValue;

const MAX_FRONTMATTER_LINES: usize = 64;
const MAX_FRONTMATTER_BYTES: usize = 16 * 1024;
const MAX_YAML_DEPTH: usize = 128;
const MAX_YAML_NODES: usize = 16 * 1024;

pub(super) fn parse_skill(request: AgentAssetParseRequest<'_>, output: &mut dyn AgentParseOutput) {
    let Some(key) = request.source.native_source_key.strip_prefix(SKILL_PREFIX) else {
        return;
    };
    let mut key_parts = key.splitn(2, ':');
    let Some(parent_source_key) = key_parts.next() else {
        return;
    };
    let Some(entry_name) = key_parts.next() else {
        return;
    };
    if !matches!(parent_source_key, "skills" | "workspace-skills") || entry_name.is_empty() {
        return;
    }
    let AgentAssetSnapshot::File { bytes, .. } = request.snapshot else {
        return;
    };
    let metadata = decode_skill_frontmatter(bytes, output);
    let state = if metadata.is_some() {
        AgentAssetDeclaredState::Enabled
    } else {
        AgentAssetDeclaredState::Unknown
    };
    let policy = metadata
        .as_ref()
        .map_or(AgentSkillInvocationPolicy::Unknown, |value| {
            value.invocation_policy
        });
    let label = metadata.and_then(|value| value.name);
    let participation = suppressed(request);
    let _ = output.emit_declaration(parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: entry_name,
            resolution_group_key: entry_name,
            category: AgentAssetCategory::Skill,
            native_id: entry_name,
            label: label.as_deref().unwrap_or(entry_name),
            logical_origin: physical_origin(request.source),
            declared_state: state,
            trust_state: declaration_trust_for_participation(
                request.context.trust_context,
                participation,
            ),
            role: crate::models::AgentAssetDeclarationRole::Definition,
            participation,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Skill {
                enabled: state,
                invocation_policy: policy,
            },
            facts: BTreeMap::new(),
        },
    ));
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::services::agent_cli::claude::environment) struct SkillFrontmatter {
    pub(in crate::services::agent_cli::claude::environment) invocation_policy:
        AgentSkillInvocationPolicy,
    pub(in crate::services::agent_cli::claude::environment) name: Option<String>,
}

impl Default for SkillFrontmatter {
    fn default() -> Self {
        Self {
            invocation_policy: AgentSkillInvocationPolicy::ModelInvocable,
            name: None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::services::agent_cli::claude::environment) enum SkillFrontmatterError {
    Encoding,
    Unterminated,
    Syntax,
    Root,
    Name,
    InvocationPolicy,
    Complexity,
    Lines { observed_at_least: usize },
    Bytes { observed_at_least: usize },
}

impl SkillFrontmatterError {
    fn diagnostic(&self) -> AgentAssetDiagnostic {
        let location = match self {
            Self::Encoding => "frontmatter.encoding",
            Self::Unterminated => "frontmatter.unterminated",
            Self::Syntax => "frontmatter.syntax",
            Self::Root => "frontmatter.root",
            Self::Name => "frontmatter.name",
            Self::InvocationPolicy => "frontmatter.disable-model-invocation",
            Self::Complexity => "frontmatter.complexity",
            Self::Lines { observed_at_least } => {
                return AgentAssetDiagnostic::Truncated {
                    limit: AgentAssetLimitKind::FrontmatterLines,
                    accepted: MAX_FRONTMATTER_LINES as u64,
                    observed_at_least: *observed_at_least as u64,
                };
            }
            Self::Bytes { observed_at_least } => {
                return AgentAssetDiagnostic::Truncated {
                    limit: AgentAssetLimitKind::FrontmatterBytes,
                    accepted: MAX_FRONTMATTER_BYTES as u64,
                    observed_at_least: *observed_at_least as u64,
                };
            }
        };
        AgentAssetDiagnostic::Malformed {
            format: AgentAssetDocumentFormat::Yaml,
            location: Some(location.to_owned()),
        }
    }
}

pub(in crate::services::agent_cli::claude::environment) fn decode_skill_frontmatter(
    bytes: &[u8],
    output: &mut dyn AgentDiagnosticOutput,
) -> Option<SkillFrontmatter> {
    match std::str::from_utf8(bytes)
        .map_err(|_| SkillFrontmatterError::Encoding)
        .and_then(skill_frontmatter)
    {
        Ok(metadata) => Some(metadata),
        Err(error) => {
            output.emit_diagnostic(error.diagnostic());
            None
        }
    }
}

pub(in crate::services::agent_cli::claude::environment) fn skill_frontmatter(
    text: &str,
) -> Result<SkillFrontmatter, SkillFrontmatterError> {
    // Preserve line endings while counting bytes. The body is deliberately not
    // passed to the YAML parser or included in the metadata budget.
    let mut lines = text.split_inclusive('\n');
    let first = lines.next().unwrap_or_default();
    if first.trim() != "---" {
        return Ok(SkillFrontmatter::default());
    }
    let mut prefix_bytes = first.len();
    let mut frontmatter = String::new();
    for (index, raw_line) in lines.enumerate() {
        if index >= MAX_FRONTMATTER_LINES {
            return Err(SkillFrontmatterError::Lines {
                observed_at_least: index + 1,
            });
        }
        prefix_bytes = prefix_bytes.saturating_add(raw_line.len());
        if prefix_bytes > MAX_FRONTMATTER_BYTES {
            return Err(SkillFrontmatterError::Bytes {
                observed_at_least: prefix_bytes,
            });
        }
        // An indented `---` is legal content in a YAML block scalar, not the
        // frontmatter delimiter. Only a column-zero delimiter ends the header.
        if raw_line.trim_end() == "---" {
            return decode_yaml_metadata(&frontmatter);
        }
        frontmatter.push_str(raw_line);
    }
    Err(SkillFrontmatterError::Unterminated)
}

fn decode_yaml_metadata(frontmatter: &str) -> Result<SkillFrontmatter, SkillFrontmatterError> {
    if frontmatter
        .lines()
        .all(|line| line.trim().is_empty() || line.trim_start().starts_with('#'))
    {
        return Ok(SkillFrontmatter::default());
    }
    // Value validates YAML mappings (including duplicate keys) without
    // coercing strings such as `"true"` into booleans. Extra native metadata
    // may be nested; it is parsed for validity but never enters public output.
    let mut document =
        serde_yaml_ng::from_str::<YamlValue>(frontmatter).map_err(yaml_metadata_error)?;
    let mut remaining_nodes = MAX_YAML_NODES;
    validate_yaml_complexity(&document, 0, &mut remaining_nodes)?;
    apply_yaml_merges_bottom_up(&mut document)?;
    let mapping = document.as_mapping().ok_or(SkillFrontmatterError::Root)?;
    let name = match mapping.get("name") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .filter(|name| !name.trim().is_empty() && !name.chars().any(char::is_control))
                .ok_or(SkillFrontmatterError::Name)?
                .to_owned(),
        ),
    };
    let invocation_policy = match mapping.get("disable-model-invocation") {
        None | Some(YamlValue::Bool(false)) => AgentSkillInvocationPolicy::ModelInvocable,
        Some(YamlValue::Bool(true)) => AgentSkillInvocationPolicy::ManualOnly,
        Some(_) => return Err(SkillFrontmatterError::InvocationPolicy),
    };
    Ok(SkillFrontmatter {
        invocation_policy,
        name,
    })
}

fn yaml_metadata_error(error: serde_yaml_ng::Error) -> SkillFrontmatterError {
    // serde_yaml_ng 0.10 exposes no typed resource-limit discriminator. Match
    // its complete fixed message and location, never a substring that could
    // occur in user metadata. Discard the library text after classification.
    let message = error.to_string();
    let limited = match error.location() {
        Some(location) if location.line() == 1 && location.column() == 1 => {
            message == "recursion limit exceeded"
        }
        Some(location) => {
            message
                == format!(
                    "recursion limit exceeded at line {} column {}",
                    location.line(),
                    location.column()
                )
        }
        None => message == "repetition limit exceeded",
    };
    if limited {
        SkillFrontmatterError::Complexity
    } else {
        SkillFrontmatterError::Syntax
    }
}

fn validate_yaml_complexity(
    value: &YamlValue,
    depth: usize,
    remaining_nodes: &mut usize,
) -> Result<(), SkillFrontmatterError> {
    if depth > MAX_YAML_DEPTH || *remaining_nodes == 0 {
        return Err(SkillFrontmatterError::Complexity);
    }
    *remaining_nodes -= 1;
    match value {
        YamlValue::Mapping(mapping) => {
            for (key, child) in mapping {
                validate_yaml_complexity(key, depth + 1, remaining_nodes)?;
                validate_yaml_complexity(child, depth + 1, remaining_nodes)?;
            }
        }
        YamlValue::Sequence(sequence) => {
            for child in sequence {
                validate_yaml_complexity(child, depth + 1, remaining_nodes)?;
            }
        }
        YamlValue::Tagged(tagged) => {
            validate_yaml_complexity(&tagged.value, depth + 1, remaining_nodes)?;
        }
        _ => {}
    }
    Ok(())
}

fn apply_yaml_merges_bottom_up(value: &mut YamlValue) -> Result<(), SkillFrontmatterError> {
    // serde_yaml_ng 0.10 visits parents before children and does not revisit a
    // `<<` inserted by a nested merge. Normalize every merge source first, then
    // use the library's own merge/precedence validation at its parent. Merges
    // move existing nodes; they cannot enlarge the prevalidated tree. Depth
    // and total nodes above bound recursion and repeated library traversals.
    let needs_merge = match value {
        YamlValue::Mapping(mapping) => {
            for child in mapping.values_mut() {
                apply_yaml_merges_bottom_up(child)?;
            }
            mapping.contains_key("<<")
        }
        YamlValue::Sequence(sequence) => {
            for child in sequence {
                apply_yaml_merges_bottom_up(child)?;
            }
            false
        }
        YamlValue::Tagged(tagged) => {
            apply_yaml_merges_bottom_up(&mut tagged.value)?;
            false
        }
        _ => false,
    };
    if needs_merge {
        value
            .apply_merge()
            .map_err(|_| SkillFrontmatterError::Syntax)?;
    }
    Ok(())
}
