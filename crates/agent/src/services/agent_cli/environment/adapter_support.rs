//! Small, schema-neutral helpers shared by Agent-owned environment adapters.
//!
//! This module may build common contracts, but it must never choose an Agent's
//! native source paths, configuration keys, precedence or resolution semantics.

use super::state_projection::details_declared_state;
use crate::{
    models::{
        AgentAssetCategory, AgentAssetDeclaredState, AgentAssetDetails, AgentAssetDiagnostic,
        AgentAssetEffectiveAvailability, AgentAssetInstallationOrigin, AgentAssetNativeRef,
        AgentAssetPresence, AgentAssetResolutionParticipation, AgentAssetResolutionRelation,
        AgentAssetScope, AgentAssetSourceKind, AgentAssetState, AgentConfigurationContext,
        AgentTrustState,
    },
    services::agent_cli::contracts::{
        AgentAssetLogicalOrigin, AgentAssetNativePayload, AgentAssetParseRequest,
        AgentAssetProjectedDraft, AgentAssetResolutionDraft, AgentAssetResolvedDraftInput,
        AgentAssetResolvedRelationships, AgentAssetSourceSpec, AgentContextDiscoveryRequest,
        AgentDiagnosticOutput, ParsedAgentAsset,
    },
};
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
    rc::Rc,
};

pub(in crate::services::agent_cli) fn default_context(
    request: AgentContextDiscoveryRequest<'_>,
    config_root: PathBuf,
    parser_version: u32,
) -> Vec<AgentConfigurationContext> {
    vec![AgentConfigurationContext {
        id: String::new(),
        environment_id: request.environment_id.to_owned(),
        agent_kind: request.agent_kind,
        config_root: config_root.to_string_lossy().into_owned(),
        profile: "default".to_owned(),
        workspace_id: request
            .workspace
            .map(|path| path.to_string_lossy().into_owned()),
        trust_context: if request.workspace.is_some() {
            request.workspace_trust
        } else {
            AgentTrustState::Unknown
        },
        parser_version,
        schema_facts: BTreeMap::new(),
        compatible_installation_ids: request
            .installations
            .iter()
            .map(|installation| installation.id.clone())
            .collect(),
    }]
}

pub(in crate::services::agent_cli) fn declaration_trust_for_participation(
    context_trust: AgentTrustState,
    participation: AgentAssetResolutionParticipation,
) -> AgentTrustState {
    match participation {
        AgentAssetResolutionParticipation::Suppressed { .. } => context_trust,
        AgentAssetResolutionParticipation::Participates
            if context_trust == AgentTrustState::Trusted =>
        {
            AgentTrustState::Trusted
        }
        AgentAssetResolutionParticipation::Participates => AgentTrustState::Unknown,
    }
}

pub(in crate::services::agent_cli) struct SourceInput<'a> {
    pub native_source_key: &'a str,
    pub label: &'a str,
    pub path: PathBuf,
    pub allowed_root: &'a Path,
    pub scope: AgentAssetScope,
    pub origin: AgentAssetInstallationOrigin,
    pub precedence: u32,
    pub sensitive: bool,
    pub source_kind: AgentAssetSourceKind,
    pub categories: &'a [AgentAssetCategory],
}

pub(in crate::services::agent_cli) fn source(input: SourceInput<'_>) -> AgentAssetSourceSpec {
    let origin = AgentAssetLogicalOrigin {
        scope: input.scope,
        precedence: input.precedence,
    };
    source_with_logical_origins(input, &[origin])
}

pub(in crate::services::agent_cli) fn source_with_logical_origins(
    input: SourceInput<'_>,
    allowed_logical_origins: &[AgentAssetLogicalOrigin],
) -> AgentAssetSourceSpec {
    let mut allowed_logical_origins = allowed_logical_origins.to_vec();
    allowed_logical_origins.sort();
    AgentAssetSourceSpec {
        path_policy: Default::default(),
        verified_physical_path: None,
        native_source_key: input.native_source_key.to_owned(),
        label: input.label.to_owned(),
        scope: input.scope,
        origin: input.origin,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        path: input.path,
        allowed_root: input.allowed_root.to_path_buf(),
        precedence: input.precedence,
        writable: matches!(
            input.scope,
            AgentAssetScope::User | AgentAssetScope::Workspace | AgentAssetScope::Local
        ),
        sensitive: input.sensitive,
        source_kind: input.source_kind,
        hook_definition_source: input.categories.contains(&AgentAssetCategory::Hook),
        categories: input.categories.to_vec(),
        allowed_logical_origins,
    }
}

pub(in crate::services::agent_cli) fn physical_origin(
    source: &AgentAssetSourceSpec,
) -> AgentAssetLogicalOrigin {
    AgentAssetLogicalOrigin {
        scope: source.scope,
        precedence: source.precedence,
    }
}

pub(in crate::services::agent_cli) struct ParsedAssetInput<'a> {
    pub declaration_key: &'a str,
    pub resolution_group_key: &'a str,
    pub category: AgentAssetCategory,
    pub native_id: &'a str,
    pub label: &'a str,
    pub logical_origin: AgentAssetLogicalOrigin,
    pub declared_state: AgentAssetDeclaredState,
    pub trust_state: AgentTrustState,
    pub role: crate::models::AgentAssetDeclarationRole,
    pub participation: crate::models::AgentAssetResolutionParticipation,
    pub provided_by: Option<AgentAssetNativeRef>,
    pub action_owner: Option<AgentAssetNativeRef>,
    pub explicitly_affected: Vec<AgentAssetNativeRef>,
    pub details: AgentAssetDetails,
    pub facts: BTreeMap<String, String>,
}

pub(in crate::services::agent_cli) fn parsed_asset(
    request: AgentAssetParseRequest<'_>,
    input: ParsedAssetInput<'_>,
) -> ParsedAgentAsset {
    let category_key = input.category.key();
    ParsedAgentAsset {
        declaration_id: super::stable_id(
            "declaration",
            &[
                request.context.id.as_str(),
                request.source.native_source_key.as_str(),
                category_key.as_str(),
                input.declaration_key,
            ],
        ),
        resolution_group_key: input.resolution_group_key.to_owned(),
        source_key: request.source.native_source_key.clone(),
        native_id: input.native_id.to_owned(),
        declaration_key: input.declaration_key.to_owned(),
        label: input.label.to_owned(),
        category: input.category,
        logical_origin: input.logical_origin,
        presence: AgentAssetPresence::Present,
        declared_state: input.declared_state,
        trust_state: input.trust_state,
        role: input.role,
        participation: input.participation,
        provided_by: input.provided_by,
        action_owner: input.action_owner,
        explicitly_affected: input.explicitly_affected,
        details: input.details,
        facts: input.facts,
        native_payload: AgentAssetNativePayload::None,
    }
}

/// Validate every declaration witness, including completion markers which do
/// not become a record contributor or a terminal-control reference.
pub(in crate::services::agent_cli) fn declaration_matches_source(
    context: &AgentConfigurationContext,
    declaration: &ParsedAgentAsset,
    source: &AgentAssetSourceSpec,
) -> bool {
    declaration.source_key == source.native_source_key
        && source.categories.contains(&declaration.category)
        && source.allows(declaration.logical_origin)
        && !declaration.declaration_key.trim().is_empty()
        && !declaration.declaration_key.chars().any(char::is_control)
        && declaration.declaration_id
            == super::stable_id(
                "declaration",
                &[
                    context.id.as_str(),
                    declaration.source_key.as_str(),
                    declaration.category.key().as_str(),
                    declaration.declaration_key.as_str(),
                ],
            )
}

pub(in crate::services::agent_cli) fn bounded_object_entries(
    object: &Map<String, Value>,
) -> impl Iterator<Item = (&str, &Value)> {
    object.iter().map(|(key, value)| (key.as_str(), value))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::services::agent_cli) enum DraftConstructionError {
    Identity,
    Members,
    Details,
    Proof,
    Relationships,
}

/// Mechanical structure only. The caller has already selected the anchor and
/// every represented/contributing declaration using its native rules.
pub(in crate::services::agent_cli) struct AgentAssetDraftStructure<'a> {
    pub anchor: &'a ParsedAgentAsset,
    pub represented: Vec<&'a ParsedAgentAsset>,
    pub contributors: Vec<&'a ParsedAgentAsset>,
    pub relationships: AgentAssetResolvedRelationships,
}

pub(in crate::services::agent_cli) fn select_draft_structure<'a>(
    anchor: &'a ParsedAgentAsset,
    represented: &[&'a ParsedAgentAsset],
    contributors: &[&'a ParsedAgentAsset],
) -> Result<AgentAssetDraftStructure<'a>, DraftConstructionError> {
    if !represented
        .iter()
        .any(|asset| asset.declaration_id == anchor.declaration_id)
        || contributors.iter().any(|asset| {
            !represented
                .iter()
                .any(|represented| represented.declaration_id == asset.declaration_id)
        })
    {
        return Err(DraftConstructionError::Members);
    }
    let mut represented = represented.to_vec();
    represented.sort_by(|left, right| left.declaration_id.cmp(&right.declaration_id));
    represented.dedup_by(|left, right| left.declaration_id == right.declaration_id);
    let mut contributors = contributors.to_vec();
    contributors.sort_by(|left, right| left.declaration_id.cmp(&right.declaration_id));
    contributors.dedup_by(|left, right| left.declaration_id == right.declaration_id);
    let relationships = aggregate_relationships(&contributors)?;
    Ok(AgentAssetDraftStructure {
        anchor,
        represented,
        contributors,
        relationships,
    })
}

pub(in crate::services::agent_cli) fn aggregate_relationships(
    assets: &[&ParsedAgentAsset],
) -> Result<AgentAssetResolvedRelationships, DraftConstructionError> {
    let provided_by = aggregate_relationship(assets, |asset| &asset.provided_by)
        .ok_or(DraftConstructionError::Relationships)?;
    let action_owner = aggregate_relationship(assets, |asset| &asset.action_owner)
        .ok_or(DraftConstructionError::Relationships)?;
    let mut explicitly_affected = assets
        .iter()
        .flat_map(|asset| asset.explicitly_affected.iter().cloned())
        .collect::<Vec<_>>();
    explicitly_affected.sort_by(|left, right| {
        left.category
            .cmp(&right.category)
            .then_with(|| left.native_id.cmp(&right.native_id))
            .then_with(|| left.qualifier.cmp(&right.qualifier))
    });
    explicitly_affected.dedup();
    Ok(AgentAssetResolvedRelationships {
        provided_by,
        action_owner,
        explicitly_affected,
    })
}

pub(in crate::services::agent_cli) fn finalize_projected_draft(
    input: AgentAssetResolvedDraftInput<'_>,
) -> Result<AgentAssetProjectedDraft, DraftConstructionError> {
    if input.projection_key.trim().is_empty()
        || input.anchor.resolution_group_key.trim().is_empty()
        || input.anchor.native_id.trim().is_empty()
        || input.anchor.role != crate::models::AgentAssetDeclarationRole::Definition
    {
        return Err(DraftConstructionError::Identity);
    }
    if details_declared_state(&input.details) != input.declared_state {
        return Err(DraftConstructionError::Details);
    }
    let mut represented = input
        .represented
        .iter()
        .map(|asset| asset.declaration_id.clone())
        .collect::<Vec<_>>();
    represented.sort();
    represented.dedup();
    let mut contributors = input
        .contributors
        .iter()
        .map(|asset| asset.declaration_id.clone())
        .collect::<Vec<_>>();
    contributors.sort();
    contributors.dedup();
    if represented.is_empty()
        || represented.iter().any(|id| id.trim().is_empty())
        || !represented.contains(&input.anchor.declaration_id)
        || contributors.iter().any(|id| !represented.contains(id))
    {
        return Err(DraftConstructionError::Members);
    }
    if !effective_proof_shape_is_valid(
        &input.state_proof.effective,
        &input.resolution,
        input.relationships.provided_by.as_ref(),
    ) || match input.resolution.terminal {
        Some(crate::models::AgentAssetResolutionTerminal::PolicyBlocked) => {
            input.effective_state != AgentAssetState::Blocked
        }
        Some(crate::models::AgentAssetResolutionTerminal::Unknown) => {
            input.effective_state != AgentAssetState::Unknown
        }
        None if input.resolution.relation == AgentAssetResolutionRelation::Replaced => {
            input.effective_state != AgentAssetState::Shadowed
        }
        None => false,
    } {
        return Err(DraftConstructionError::Proof);
    }
    Ok(AgentAssetProjectedDraft {
        projection_key: input.projection_key,
        resolution_group_key: input.anchor.resolution_group_key.clone(),
        native_kind: input.anchor.category,
        native_id: input.anchor.native_id.clone(),
        label: input.anchor.label.clone(),
        declared_state: input.declared_state,
        effective_state: input.effective_state,
        trust_state: input.trust_state,
        inspection_source_id: input.anchor.source_key.clone(),
        represented_declaration_ids: represented,
        contributor_ids: contributors,
        resolution: input.resolution,
        details: input.details,
        provided_by: input.relationships.provided_by,
        action_owner: input.relationships.action_owner,
        explicitly_affected: input.relationships.explicitly_affected,
        state_proof: input.state_proof,
    })
}

pub(super) fn effective_proof_shape_is_valid(
    proof: &crate::services::agent_cli::contracts::AgentAssetEffectiveStateProofDraft,
    resolution: &AgentAssetResolutionDraft,
    provided_by: Option<&AgentAssetNativeRef>,
) -> bool {
    use crate::models::AgentAssetResolutionTerminal;
    use crate::services::agent_cli::contracts::{
        AgentAssetEffectiveStateProofDraft as Proof, AgentAssetTerminalCauseDraft as Cause,
    };
    let intrinsic_or_parent = |input: &Proof| match input {
        Proof::Intrinsic => true,
        Proof::ParentGate { parent, input } => {
            provided_by == Some(parent) && matches!(input.as_ref(), Proof::Intrinsic)
        }
        _ => false,
    };
    if resolution.relation == AgentAssetResolutionRelation::Replaced {
        return matches!(proof, Proof::Shadowed { winner, input }
            if resolution.winner.as_ref() == Some(winner) && matches!(input.as_ref(), Proof::Intrinsic))
            && resolution.terminal.is_none()
            && resolution.control_source.is_none();
    }
    match proof {
        Proof::Intrinsic | Proof::ParentGate { .. } => {
            resolution.terminal.is_none()
                && resolution.control_source.is_none()
                && intrinsic_or_parent(proof)
        }
        Proof::Terminal {
            terminal,
            cause,
            evidence,
            input,
        } => {
            if resolution.terminal != Some(*terminal) || !intrinsic_or_parent(input) {
                return false;
            }
            match cause {
                Cause::TypedPolicy => {
                    *terminal == AgentAssetResolutionTerminal::PolicyBlocked && !evidence.is_empty()
                }
                Cause::InvalidControl => {
                    *terminal == AgentAssetResolutionTerminal::Unknown && !evidence.is_empty()
                }
                Cause::ParentUnknown => {
                    *terminal == AgentAssetResolutionTerminal::Unknown
                        && evidence.is_empty()
                        && resolution.control_source.is_none()
                        && matches!(input.as_ref(), Proof::ParentGate { .. })
                }
                Cause::DeclaredUnknown | Cause::StructuralUnknown | Cause::TrustSuppressed => {
                    *terminal == AgentAssetResolutionTerminal::Unknown
                        && evidence.is_empty()
                        && resolution.control_source.is_none()
                }
            }
        }
        Proof::Shadowed { .. } => false,
    }
}

fn aggregate_relationship<F>(
    assets: &[&ParsedAgentAsset],
    relationship: F,
) -> Option<Option<AgentAssetNativeRef>>
where
    F: Fn(&ParsedAgentAsset) -> &Option<AgentAssetNativeRef>,
{
    let mut selected = None;
    for asset in assets {
        let Some(value) = relationship(asset) else {
            continue;
        };
        if selected.as_ref().is_some_and(|existing| existing != value) {
            return None;
        }
        selected = Some(value.clone());
    }
    Some(selected)
}

pub(in crate::services::agent_cli) fn qualified_projection_key(asset: &ParsedAgentAsset) -> String {
    format!(
        "{}:{}@{}:{}",
        asset.category.key(),
        asset.native_id,
        asset.source_key,
        asset.declaration_id
    )
}

pub(in crate::services::agent_cli) fn state_from_declared(
    state: AgentAssetDeclaredState,
) -> AgentAssetState {
    match state {
        AgentAssetDeclaredState::Enabled => AgentAssetState::Enabled,
        AgentAssetDeclaredState::Disabled => AgentAssetState::Disabled,
        AgentAssetDeclaredState::Rejected => AgentAssetState::Blocked,
        AgentAssetDeclaredState::Pending | AgentAssetDeclaredState::Unknown => {
            AgentAssetState::Unknown
        }
    }
}

pub(in crate::services::agent_cli) fn availability_from_declared(
    state: AgentAssetDeclaredState,
) -> AgentAssetEffectiveAvailability {
    match state {
        AgentAssetDeclaredState::Enabled => AgentAssetEffectiveAvailability::Available,
        AgentAssetDeclaredState::Disabled => AgentAssetEffectiveAvailability::Disabled,
        AgentAssetDeclaredState::Pending => AgentAssetEffectiveAvailability::ApprovalRequired,
        AgentAssetDeclaredState::Rejected => AgentAssetEffectiveAvailability::PolicyBlocked,
        AgentAssetDeclaredState::Unknown => AgentAssetEffectiveAvailability::Unknown,
    }
}

pub(in crate::services::agent_cli) fn parse_json(
    bytes: &[u8],
    duplicate_category: Option<AgentAssetCategory>,
    output: &mut dyn AgentDiagnosticOutput,
) -> Result<Value, ()> {
    let output = Rc::new(RefCell::new(output));
    let seed = JsonValueSeed {
        path: String::new(),
        duplicate_category,
        output: Rc::clone(&output),
    };
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = seed.deserialize(&mut deserializer).map_err(|_| ())?;
    deserializer.end().map_err(|_| ())?;
    Ok(value)
}

pub(in crate::services::agent_cli) fn emit_unknown_fields(
    object: &Map<String, Value>,
    known: &[&str],
    prefix: &str,
    output: &mut dyn AgentDiagnosticOutput,
) {
    for key in object.keys().filter(|key| !known.contains(&key.as_str())) {
        if !output.has_regular_capacity() {
            return;
        }
        output.emit_diagnostic(AgentAssetDiagnostic::UnknownField {
            field_path: if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            },
        });
    }
}

struct JsonValueSeed<'a> {
    path: String,
    duplicate_category: Option<AgentAssetCategory>,
    output: Rc<RefCell<&'a mut dyn AgentDiagnosticOutput>>,
}

impl<'de, 'a> DeserializeSeed<'de> for JsonValueSeed<'a> {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(JsonValueVisitor {
            path: self.path,
            duplicate_category: self.duplicate_category,
            output: self.output,
        })
    }
}

struct JsonValueVisitor<'a> {
    path: String,
    duplicate_category: Option<AgentAssetCategory>,
    output: Rc<RefCell<&'a mut dyn AgentDiagnosticOutput>>,
}

impl<'a> JsonValueVisitor<'a> {
    fn child(&self, key: &str) -> JsonValueSeed<'a> {
        JsonValueSeed {
            path: if self.path.is_empty() {
                key.to_owned()
            } else {
                format!("{}.{}", self.path, key)
            },
            duplicate_category: self.duplicate_category,
            output: Rc::clone(&self.output),
        }
    }
}

impl<'de, 'a> Visitor<'de> for JsonValueVisitor<'a> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("invalid JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(Value::String(value))
    }

    fn visit_seq<A>(self, mut values: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut result = Vec::new();
        while let Some(value) = values.next_element_seed(self.child(&result.len().to_string()))? {
            result.push(value);
        }
        Ok(Value::Array(result))
    }

    fn visit_map<A>(self, mut values: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut result = Map::new();
        while let Some(key) = values.next_key::<String>()? {
            let duplicate_mcp = result.contains_key(&key)
                && self.path == "mcpServers"
                && self.duplicate_category.is_some();
            let value = values.next_value_seed(self.child(&key))?;
            if duplicate_mcp {
                let mut output = self.output.borrow_mut();
                if output.has_regular_capacity() {
                    if let Some(category) = self.duplicate_category {
                        output.emit_diagnostic(AgentAssetDiagnostic::DuplicateNativeId {
                            category,
                            native_id: key.clone(),
                        });
                    }
                }
            }
            result.insert(key, value);
        }
        Ok(Value::Object(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        AgentAssetDeclaredState, AgentAssetDetails, AgentAssetPresence,
        AgentAssetResolutionParticipation, AgentSkillInvocationPolicy, AgentTrustState,
    };

    #[derive(Default)]
    struct TestDiagnostics(Vec<AgentAssetDiagnostic>);

    impl AgentDiagnosticOutput for TestDiagnostics {
        fn has_regular_capacity(&self) -> bool {
            true
        }

        fn emit_diagnostic(
            &mut self,
            value: AgentAssetDiagnostic,
        ) -> crate::services::agent_cli::contracts::AgentDiagnosticEmission {
            self.0.push(value);
            crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
        }
    }

    struct SaturatingDiagnostics {
        remaining: usize,
        emission_calls: usize,
        values: Vec<AgentAssetDiagnostic>,
    }

    impl SaturatingDiagnostics {
        fn with_capacity(remaining: usize) -> Self {
            Self {
                remaining,
                emission_calls: 0,
                values: Vec::new(),
            }
        }
    }

    impl AgentDiagnosticOutput for SaturatingDiagnostics {
        fn has_regular_capacity(&self) -> bool {
            self.remaining > 0
        }

        fn emit_diagnostic(
            &mut self,
            value: AgentAssetDiagnostic,
        ) -> crate::services::agent_cli::contracts::AgentDiagnosticEmission {
            self.emission_calls += 1;
            if self.remaining == 0 {
                return crate::services::agent_cli::contracts::AgentDiagnosticEmission::Saturated;
            }
            self.remaining -= 1;
            self.values.push(value);
            crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
        }
    }

    fn relationship_asset(
        declaration_id: &str,
        provided_by: Option<AgentAssetNativeRef>,
        action_owner: Option<AgentAssetNativeRef>,
        explicitly_affected: Vec<AgentAssetNativeRef>,
    ) -> ParsedAgentAsset {
        ParsedAgentAsset {
            declaration_id: declaration_id.to_owned(),
            resolution_group_key: "merged".to_owned(),
            source_key: "source".to_owned(),
            logical_origin: crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: crate::models::AgentAssetScope::User,
                precedence: 1,
            },
            native_id: "asset".to_owned(),
            declaration_key: declaration_id.to_owned(),
            label: declaration_id.to_owned(),
            category: AgentAssetCategory::Skill,
            presence: AgentAssetPresence::Present,
            declared_state: AgentAssetDeclaredState::Enabled,
            trust_state: AgentTrustState::Unknown,
            role: crate::models::AgentAssetDeclarationRole::Definition,
            participation: AgentAssetResolutionParticipation::Participates,
            provided_by,
            action_owner,
            explicitly_affected,
            details: AgentAssetDetails::Skill {
                enabled: AgentAssetDeclaredState::Enabled,
                invocation_policy: AgentSkillInvocationPolicy::Unknown,
            },
            facts: BTreeMap::new(),
            native_payload: AgentAssetNativePayload::None,
        }
    }

    fn relationship(native_id: &str) -> AgentAssetNativeRef {
        AgentAssetNativeRef {
            category: AgentAssetCategory::Plugin,
            native_id: native_id.to_owned(),
            qualifier: Some(format!("plugin:{native_id}")),
        }
    }

    #[test]
    fn merged_relationships_ignore_empty_overlay_edges_and_deduplicate_impacts() {
        let owner = relationship("provider");
        let child = relationship("child");
        let definition = relationship_asset(
            "definition",
            Some(owner.clone()),
            Some(owner.clone()),
            vec![child.clone()],
        );
        let overlay = relationship_asset("overlay", None, None, vec![child.clone()]);
        let assets = vec![&definition, &overlay];
        let relationships =
            aggregate_relationships(&assets).expect("consistent aggregate relationships");
        assert_eq!(relationships.provided_by, Some(owner.clone()));
        assert_eq!(relationships.action_owner, Some(owner));
        assert_eq!(relationships.explicitly_affected, vec![child]);
    }

    #[test]
    fn merged_relationships_reject_conflicting_non_empty_edges() {
        let definition = relationship_asset(
            "definition",
            Some(relationship("provider-a")),
            Some(relationship("provider-a")),
            Vec::new(),
        );
        let overlay = relationship_asset(
            "overlay",
            Some(relationship("provider-b")),
            None,
            Vec::new(),
        );
        let assets = vec![&definition, &overlay];
        assert_eq!(
            aggregate_relationships(&assets),
            Err(DraftConstructionError::Relationships)
        );
    }

    #[test]
    fn duplicate_json_keys_are_reported_before_map_materialization() {
        let mut diagnostics = TestDiagnostics::default();
        let value = parse_json(
            br#"{"mcpServers":{"same":{"command":"one"},"same":{"command":"two"}}}"#,
            Some(AgentAssetCategory::Mcp),
            &mut diagnostics,
        )
        .expect("fixture should parse");
        assert_eq!(value["mcpServers"]["same"]["command"], "two");
        assert!(matches!(
            diagnostics.0.as_slice(),
            [AgentAssetDiagnostic::DuplicateNativeId { native_id, .. }] if native_id == "same"
        ));
    }

    #[test]
    fn duplicate_json_diagnostics_stop_dynamic_work_after_capacity_is_exhausted() {
        let mut diagnostics = SaturatingDiagnostics::with_capacity(1);
        parse_json(
            br#"{"mcpServers":{"same":{"command":"one"},"same":{"command":"two"},"same":{"command":"three"}}}"#,
            Some(AgentAssetCategory::Mcp),
            &mut diagnostics,
        )
        .expect("fixture should parse");
        assert_eq!(diagnostics.emission_calls, 1);
        assert_eq!(diagnostics.values.len(), 1);
        assert!(!diagnostics.has_regular_capacity());
    }
}
