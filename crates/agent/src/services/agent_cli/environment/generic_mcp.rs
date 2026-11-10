//! Read-only MCP inventory for integration-tier Agents.
//!
//! These Agents expose native configuration and, where the CLI supports it, a
//! temporary launch plan, but do not publish a full Asset inventory. Their MCP
//! declarations still live in ordinary native documents, so this module reads
//! the well-known MCP sections without inventing per-Agent enablement rules.
//!
//! Only declarations that name a native command or URL are admitted. A
//! document that has no recognized MCP section simply contributes nothing;
//! this is a bounded observation, not a claim that the Agent supports MCP.

use super::{
    adapter_support::{
        bounded_object_entries, finalize_projected_draft, parsed_asset, physical_origin,
        qualified_projection_key, source, ParsedAssetInput, SourceInput,
    },
    state_projection::{compose_effective_state, expected_mcp_availability, intrinsic_basis},
};
use crate::{
    models::{
        AgentAssetCategory, AgentAssetDeclarationRole, AgentAssetDeclaredState, AgentAssetDetails,
        AgentAssetDiagnostic, AgentAssetDocumentFormat, AgentAssetNativeRef,
        AgentAssetResolutionParticipation, AgentAssetResolutionRelation, AgentAssetScope,
        AgentAssetSourceKind, AgentCliKind, AgentMcpApprovalState, AgentMcpTransport,
    },
    services::agent_cli::{
        config_support::{parse_jsonc_document, text_document, yaml_source},
        contracts::{
            AgentAssetAssessmentIndex, AgentAssetAssessmentRequest,
            AgentAssetDeclaredStateProofDraft, AgentAssetEffectiveStateProofDraft,
            AgentAssetParseRequest, AgentAssetResolutionDraft, AgentAssetResolveRequest,
            AgentAssetResolvedDraftInput, AgentAssetResolvedRelationships, AgentAssetSnapshot,
            AgentAssetStateProofDraft, AgentParseOutput, AgentResolveOutput,
            AgentSourceDiscoveryRequest, InitialSourceOutput, ParsedAgentAsset,
        },
    },
};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, path::Path};

/// Native files that may carry MCP declarations. The list is deliberately
/// limited to filenames already used by the supported CLIs; adding a filename
/// here never grants write access or executes anything found in the file.
const MCP_FILES: &[(&str, u32)] = &[
    ("mcp.json", 20),
    ("mcp_config.json", 20),
    ("mcp-config.json", 20),
    ("settings.json", 10),
    ("settings.jsonc", 10),
    ("settings.yaml", 10),
    ("settings.yml", 10),
    ("config.json", 10),
    ("config.jsonc", 10),
    ("config.yaml", 10),
    ("config.yml", 10),
    ("config.toml", 10),
    ("opencode.json", 10),
    ("opencode.jsonc", 10),
    ("crush.json", 10),
    ("openclaw.json", 10),
    ("mimocode.json", 10),
    ("mimocode.jsonc", 10),
    ("deveco.json", 10),
    ("deveco.jsonc", 10),
    (".project_config.toml", 10),
];

pub(in crate::services::agent_cli) fn discover_sources(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let root = Path::new(&request.context.config_root);
    for (name, precedence) in MCP_FILES {
        let path = root.join(name);
        if !path.is_file() {
            continue;
        }
        let spec = source(SourceInput {
            origin: crate::models::AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: name,
            label: "MCP 配置",
            path,
            allowed_root: root,
            scope: AgentAssetScope::User,
            precedence: *precedence,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[AgentAssetCategory::Mcp],
        });
        if output.emit_initial(spec).is_break() {
            return;
        }
    }
}

pub(in crate::services::agent_cli) fn parse_assets(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    if !request.source.categories.contains(&AgentAssetCategory::Mcp) {
        return;
    }
    let AgentAssetSnapshot::File { bytes, .. } = request.snapshot else {
        return;
    };
    let Ok(text) = text_document::decode(bytes) else {
        return;
    };
    let Some(document) = parse_document(text, &request.source.path) else {
        output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
            format: document_format(&request.source.path),
            location: None,
        });
        return;
    };
    let Some(root) = document.as_object() else {
        return;
    };
    for (section, key) in mcp_sections(root, request.context.agent_kind) {
        let Some(entries) = section.as_object() else {
            continue;
        };
        for (native_id, entry) in bounded_object_entries(entries) {
            let Some(declaration) = declaration(request, key, native_id, entry) else {
                continue;
            };
            if output.emit_declaration(declaration).is_break() {
                return;
            }
        }
    }
}

fn mcp_sections(
    root: &Map<String, Value>,
    agent_kind: AgentCliKind,
) -> Vec<(&Value, &'static str)> {
    let mut sections = Vec::new();
    for key in ["mcpServers", "mcp_servers", "mcp"] {
        if let Some(value) = root.get(key) {
            sections.push((value, key));
        }
    }
    // Goose models MCP extensions under `extensions`. Other YAML/TOML
    // documents commonly use that word for unrelated capability lists, so the
    // generic reader only admits it for the Agent whose native schema defines
    // it that way.
    if agent_kind == AgentCliKind::Goose {
        if let Some(value) = root.get("extensions") {
            sections.push((value, "extensions"));
        }
    }
    sections
}

fn parse_document(text: &str, path: &Path) -> Option<Value> {
    match document_format(path) {
        AgentAssetDocumentFormat::Yaml => yaml_source::value(text).ok(),
        AgentAssetDocumentFormat::Toml => toml::from_str::<toml::Value>(text)
            .ok()
            .and_then(|value| serde_json::to_value(value).ok()),
        AgentAssetDocumentFormat::Json => parse_jsonc_document(text).ok(),
        AgentAssetDocumentFormat::Manifest | AgentAssetDocumentFormat::Unknown => None,
    }
}

fn document_format(path: &Path) -> AgentAssetDocumentFormat {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "yaml" | "yml" => AgentAssetDocumentFormat::Yaml,
        "toml" => AgentAssetDocumentFormat::Toml,
        _ => AgentAssetDocumentFormat::Json,
    }
}

fn declaration(
    request: AgentAssetParseRequest<'_>,
    section: &str,
    native_id: &str,
    entry: &Value,
) -> Option<ParsedAgentAsset> {
    let entry = entry.as_object()?;
    let transport = mcp_transport(entry)?;
    let declared_state = declared_state(entry);
    let availability = expected_mcp_availability(
        declared_state,
        AgentMcpApprovalState::NotRequired,
        request.context.trust_context,
    );
    let label = entry
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(native_id);
    let declaration_key = format!("{section}.{native_id}");
    let resolution_group_key = format!("mcp:{native_id}");
    let mut asset = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: &declaration_key,
            resolution_group_key: &resolution_group_key,
            category: AgentAssetCategory::Mcp,
            native_id,
            label,
            logical_origin: physical_origin(request.source),
            declared_state,
            trust_state: request.context.trust_context,
            role: AgentAssetDeclarationRole::Definition,
            participation: AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Mcp {
                transport,
                declared_state,
                approval_state: AgentMcpApprovalState::NotRequired,
                effective_availability: availability,
            },
            facts: BTreeMap::new(),
        },
    );
    asset.facts.insert("section".into(), section.to_owned());
    Some(asset)
}

fn declared_state(entry: &Map<String, Value>) -> AgentAssetDeclaredState {
    if entry.get("disabled").and_then(Value::as_bool) == Some(true)
        || entry.get("enabled").and_then(Value::as_bool) == Some(false)
    {
        AgentAssetDeclaredState::Disabled
    } else {
        AgentAssetDeclaredState::Enabled
    }
}

fn mcp_transport(entry: &Map<String, Value>) -> Option<AgentMcpTransport> {
    let explicit = entry
        .get("type")
        .or_else(|| entry.get("transport"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_ascii_lowercase());
    let has_command = command_value(entry).is_some();
    let has_url = url_value(entry).is_some();
    match explicit.as_deref() {
        Some("stdio" | "local") if has_command => Some(AgentMcpTransport::Stdio),
        Some("http" | "remote" | "streamable-http" | "streamable_http") if has_url => {
            Some(AgentMcpTransport::Http)
        }
        Some("sse") if has_url => Some(AgentMcpTransport::Sse),
        Some("ws" | "websocket") if has_url => Some(AgentMcpTransport::WebSocket),
        Some("builtin") => None,
        Some(_) => None,
        None if has_command => Some(AgentMcpTransport::Stdio),
        None if has_url => Some(AgentMcpTransport::Http),
        None => None,
    }
}

fn command_value(entry: &Map<String, Value>) -> Option<&Value> {
    entry.get("command").or_else(|| entry.get("cmd"))
}

fn url_value(entry: &Map<String, Value>) -> Option<&Value> {
    ["url", "serverUrl", "httpUrl", "uri"]
        .iter()
        .find_map(|key| entry.get(*key))
        .filter(|value| value.as_str().is_some_and(|value| !value.trim().is_empty()))
}

pub(in crate::services::agent_cli) fn resolve_assets(
    request: AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    let mut groups: BTreeMap<(AgentAssetCategory, String), Vec<&ParsedAgentAsset>> =
        BTreeMap::new();
    for declaration in request.declarations {
        if declaration.category != AgentAssetCategory::Mcp
            || declaration.participation != AgentAssetResolutionParticipation::Participates
        {
            continue;
        }
        groups
            .entry((
                declaration.category,
                declaration.resolution_group_key.clone(),
            ))
            .or_default()
            .push(declaration);
    }
    for ((category, _), mut members) in groups {
        members.sort_by(|left, right| {
            left.logical_origin
                .precedence
                .cmp(&right.logical_origin.precedence)
                .then_with(|| left.source_key.cmp(&right.source_key))
                .then_with(|| left.declaration_id.cmp(&right.declaration_id))
        });
        let winner = *members.last().expect("non-empty group");
        let winner_ref = native_ref(winner);
        let multiple = members.len() > 1;
        for member in &members {
            let is_winner = member.declaration_id == winner.declaration_id;
            let relation = if is_winner {
                if multiple {
                    AgentAssetResolutionRelation::ReplaceWinner
                } else {
                    AgentAssetResolutionRelation::Independent
                }
            } else {
                AgentAssetResolutionRelation::Replaced
            };
            let effective_proof = if is_winner {
                AgentAssetEffectiveStateProofDraft::Intrinsic
            } else {
                AgentAssetEffectiveStateProofDraft::Shadowed {
                    winner: winner_ref.clone(),
                    input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
                }
            };
            let Some((effective_state, details)) = compose_effective_state(
                &intrinsic_basis(member.details.clone(), member.trust_state),
                &effective_proof,
                None,
            ) else {
                continue;
            };
            let represented = if is_winner {
                members.to_vec()
            } else {
                vec![*member]
            };
            let contributors = vec![*member];
            let projection_key = if multiple && !is_winner {
                qualified_projection_key(member)
            } else {
                format!("{}:{}", category.key(), member.native_id)
            };
            let resolution = AgentAssetResolutionDraft {
                relation,
                qualified_collision: false,
                terminal: None,
                winner: multiple.then_some(winner_ref.clone()),
                control_source: None,
            };
            let state_proof = AgentAssetStateProofDraft {
                declared: AgentAssetDeclaredStateProofDraft::Definition {
                    declaration_ids: vec![member.declaration_id.clone()],
                    selected_id: member.declaration_id.clone(),
                },
                effective: effective_proof.clone(),
            };
            let Ok(draft) = finalize_projected_draft(AgentAssetResolvedDraftInput {
                anchor: member,
                projection_key,
                represented: &represented,
                contributors: &contributors,
                declared_state: member.declared_state,
                effective_state,
                trust_state: member.trust_state,
                resolution,
                details,
                relationships: AgentAssetResolvedRelationships::default(),
                state_proof,
            }) else {
                output.emit_diagnostic(AgentAssetDiagnostic::InvalidResolution {
                    projection_key: format!("{}:{}", member.category.key(), member.native_id),
                    resolution: relation,
                });
                continue;
            };
            if output.emit_draft(draft).is_break() {
                return;
            }
        }
    }
}

fn native_ref(asset: &ParsedAgentAsset) -> AgentAssetNativeRef {
    AgentAssetNativeRef {
        category: asset.category,
        native_id: asset.native_id.clone(),
        qualifier: None,
    }
}

pub(in crate::services::agent_cli) fn assess_assets(
    _request: AgentAssetAssessmentRequest<'_>,
) -> AgentAssetAssessmentIndex {
    AgentAssetAssessmentIndex::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::{AgentAssetRevision, AgentConfigurationContext, AgentTrustState},
        services::agent_cli::contracts::{
            AgentAssetProjectedDraft, AgentAssetSourceSpec, AgentDiagnosticEmission,
            AgentDiagnosticOutput, AgentOutputStop, AgentParseOutput, AgentResolveOutput,
        },
    };
    use std::{cell::RefCell, ops::ControlFlow, rc::Rc};

    struct ParseCollector {
        declarations: Rc<RefCell<Vec<ParsedAgentAsset>>>,
    }

    impl AgentDiagnosticOutput for ParseCollector {
        fn has_regular_capacity(&self) -> bool {
            true
        }

        fn emit_diagnostic(&mut self, _value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
            AgentDiagnosticEmission::Accepted
        }
    }

    impl AgentParseOutput for ParseCollector {
        fn emit_declaration(&mut self, value: ParsedAgentAsset) -> ControlFlow<AgentOutputStop> {
            self.declarations.borrow_mut().push(value);
            ControlFlow::Continue(())
        }
    }

    struct ResolveCollector {
        drafts: Rc<RefCell<Vec<AgentAssetProjectedDraft>>>,
    }

    impl AgentDiagnosticOutput for ResolveCollector {
        fn has_regular_capacity(&self) -> bool {
            true
        }

        fn emit_diagnostic(&mut self, _value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
            AgentDiagnosticEmission::Accepted
        }
    }

    impl AgentResolveOutput for ResolveCollector {
        fn emit_draft(&mut self, value: AgentAssetProjectedDraft) -> ControlFlow<AgentOutputStop> {
            self.drafts.borrow_mut().push(value);
            ControlFlow::Continue(())
        }
    }

    fn context(kind: AgentCliKind) -> AgentConfigurationContext {
        AgentConfigurationContext {
            id: "context".into(),
            environment_id: "native".into(),
            agent_kind: kind,
            config_root: "/tmp/agent".into(),
            profile: "default".into(),
            workspace_id: None,
            trust_context: AgentTrustState::Unknown,
            parser_version: 1,
            schema_facts: BTreeMap::new(),
            compatible_installation_ids: Vec::new(),
        }
    }

    fn spec(name: &str, precedence: u32) -> AgentAssetSourceSpec {
        source(SourceInput {
            origin: crate::models::AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: name,
            label: "MCP 配置",
            path: Path::new("/tmp/agent").join(name),
            allowed_root: Path::new("/tmp/agent"),
            scope: AgentAssetScope::User,
            precedence,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[AgentAssetCategory::Mcp],
        })
    }

    fn parse(kind: AgentCliKind, name: &str, text: &str) -> Vec<ParsedAgentAsset> {
        let context = context(kind);
        let source = spec(name, 10);
        let snapshot = AgentAssetSnapshot::File {
            bytes: text.as_bytes().to_vec(),
            revision: AgentAssetRevision::default(),
        };
        let declarations = Rc::new(RefCell::new(Vec::new()));
        let mut output = ParseCollector {
            declarations: declarations.clone(),
        };
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &source,
                snapshot: &snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut output,
        );
        let value = declarations.borrow().clone();
        value
    }

    #[test]
    fn parses_claude_style_mcp_servers() {
        let declarations = parse(
            AgentCliKind::Qoder,
            "settings.json",
            r#"{"mcpServers":{"fetch":{"command":"npx","args":["-y","fetch"]},"remote":{"url":"https://mcp.example.com","type":"http"}}}"#,
        );
        assert_eq!(declarations.len(), 2);
        let fetch = declarations
            .iter()
            .find(|asset| asset.native_id == "fetch")
            .unwrap();
        assert_eq!(
            fetch.details,
            AgentAssetDetails::Mcp {
                transport: AgentMcpTransport::Stdio,
                declared_state: AgentAssetDeclaredState::Enabled,
                approval_state: AgentMcpApprovalState::NotRequired,
                effective_availability: crate::models::AgentAssetEffectiveAvailability::Available,
            }
        );
    }

    #[test]
    fn parses_opencode_style_command_arrays_and_disabled_state() {
        let declarations = parse(
            AgentCliKind::OpenCode,
            "opencode.json",
            r#"{"mcp":{"local":{"type":"local","command":["bun","run","server"],"enabled":false}}}"#,
        );
        assert_eq!(declarations.len(), 1);
        assert_eq!(
            declarations[0].declared_state,
            AgentAssetDeclaredState::Disabled
        );
        assert!(matches!(
            declarations[0].details,
            AgentAssetDetails::Mcp {
                transport: AgentMcpTransport::Stdio,
                ..
            }
        ));
    }

    #[test]
    fn parses_goose_extensions_but_not_other_yaml_extensions() {
        let text = "extensions:\n  developer:\n    type: builtin\n  fetch:\n    type: stdio\n    cmd: npx\n    args: [\"-y\", \"fetch\"]\n";
        assert!(parse(AgentCliKind::Goose, "config.yaml", text).len() == 1);
        assert!(parse(AgentCliKind::Dsh, "config.yaml", text).is_empty());
    }

    #[test]
    fn resolves_duplicate_native_ids_to_the_higher_precedence_source() {
        let declarations = {
            let mut values = parse(
                AgentCliKind::Qoder,
                "settings.json",
                r#"{"mcpServers":{"fetch":{"command":"npx"}}}"#,
            );
            let context = context(AgentCliKind::Qoder);
            let higher = spec("mcp.json", 20);
            let snapshot = AgentAssetSnapshot::File {
                bytes: br#"{"mcpServers":{"fetch":{"command":"npx","args":["-y"]}}}"#.to_vec(),
                revision: AgentAssetRevision::default(),
            };
            let mut output = ParseCollector {
                declarations: Rc::new(RefCell::new(Vec::new())),
            };
            parse_assets(
                AgentAssetParseRequest {
                    native_home: None,
                    context: &context,
                    source: &higher,
                    snapshot: &snapshot,
                    workspace_canonical: None,
                    workspace_lexical: None,
                },
                &mut output,
            );
            // `parse_assets` moved the parsed values through the collector.
            // Re-run the lower-precedence declaration with the same owned data.
            values.extend(output.declarations.borrow().clone());
            values
        };
        let context = context(AgentCliKind::Qoder);
        let sources = [spec("settings.json", 10), spec("mcp.json", 20)];
        let source_refs = sources
            .iter()
            .map(|_source| AgentAssetSnapshot::Missing {
                revision: AgentAssetRevision::default(),
            })
            .collect::<Vec<_>>();
        let resolve_sources = sources
            .iter()
            .zip(&source_refs)
            .map(|(spec, snapshot)| {
                crate::services::agent_cli::contracts::AgentAssetResolveSource { spec, snapshot }
            })
            .collect::<Vec<_>>();
        let drafts = Rc::new(RefCell::new(Vec::new()));
        let mut output = ResolveCollector {
            drafts: drafts.clone(),
        };
        resolve_assets(
            AgentAssetResolveRequest {
                context: &context,
                declarations: &declarations,
                sources: &resolve_sources,
            },
            &mut output,
        );
        let drafts = drafts.borrow();
        assert_eq!(drafts.len(), 2);
        let winner = drafts
            .iter()
            .find(|draft| draft.effective_state == crate::models::AgentAssetState::Enabled)
            .unwrap();
        assert_eq!(
            winner.resolution.relation,
            AgentAssetResolutionRelation::ReplaceWinner
        );
        assert!(drafts
            .iter()
            .any(|draft| draft.effective_state == crate::models::AgentAssetState::Shadowed));
    }
}
