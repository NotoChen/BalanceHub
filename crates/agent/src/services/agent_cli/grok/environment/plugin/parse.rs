use super::{
    incomplete, manifest, revision, Evidence, GrokPluginPayload, Parent, ParentEvidence, SkillRole,
    SourceKey, SourceKind, CONVENTIONS, PROBE_ID, SOURCE_PREFIX,
};
use crate::models::{
    AgentAssetCategory, AgentAssetDeclarationRole, AgentAssetDeclaredState, AgentAssetDetails,
    AgentAssetDiagnostic, AgentAssetDiscoveryIncompleteReason, AgentAssetDocumentFormat,
    AgentAssetInstallState, AgentAssetPresence, AgentAssetResolutionParticipation, AgentAssetScope,
    AgentAssetSourceKind, AgentMcpApprovalState, AgentTrustState,
};
use crate::services::agent_cli::contracts::{
    AgentAssetNativePayload, AgentAssetParseRequest, AgentAssetSnapshot, AgentDiagnosticOutput,
    AgentParseOutput, ParsedAgentAsset,
};
use crate::services::agent_cli::environment::{
    availability_from_declared, bounded_object_entries,
    config_document::{self, ConfigDocumentFormat},
    parse_json, parsed_asset, physical_origin, ParsedAssetInput,
};
use std::{collections::BTreeMap, ops::ControlFlow, path::Path, sync::Arc};

pub(super) fn package_trust(
    scope: AgentAssetScope,
    context_trust: AgentTrustState,
) -> AgentTrustState {
    if scope == AgentAssetScope::User {
        AgentTrustState::Trusted
    } else {
        context_trust
    }
}

fn malformed(output: &mut dyn AgentDiagnosticOutput, format: AgentAssetDocumentFormat) {
    output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
        format,
        location: None,
    });
}

fn payload(request: AgentAssetParseRequest<'_>, evidence: Evidence) -> AgentAssetNativePayload {
    AgentAssetNativePayload::GrokPlugin(GrokPluginPayload {
        revision: revision(request.snapshot).to_owned(),
        evidence,
    })
}

fn probe(
    request: AgentAssetParseRequest<'_>,
    evidence: Evidence,
    declaration_key: &str,
) -> ParsedAgentAsset {
    let mut asset = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key,
            resolution_group_key: PROBE_ID,
            category: AgentAssetCategory::Plugin,
            native_id: PROBE_ID,
            label: "Grok Plugin 发现证据",
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: AgentTrustState::Unknown,
            role: AgentAssetDeclarationRole::PolicyOverlay,
            participation: AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Plugin {
                install_state: AgentAssetInstallState::Unknown,
                enabled: AgentAssetDeclaredState::Unknown,
                trusted: AgentTrustState::Unknown,
            },
            facts: BTreeMap::new(),
        },
    );
    if matches!(evidence, Evidence::MissingManifest) {
        asset.presence = AgentAssetPresence::Missing;
    }
    asset.native_payload = payload(request, evidence);
    asset
}

fn package(
    request: AgentAssetParseRequest<'_>,
    descriptor: &manifest::Descriptor,
    evidence: Evidence,
) -> ParsedAgentAsset {
    let trusted = package_trust(request.source.scope, request.context.trust_context);
    let mut asset = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: "plugin",
            resolution_group_key: &descriptor.namespace,
            category: AgentAssetCategory::Plugin,
            native_id: &descriptor.namespace,
            label: &descriptor.namespace,
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: trusted,
            role: AgentAssetDeclarationRole::Definition,
            participation: AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Plugin {
                install_state: AgentAssetInstallState::Installed,
                enabled: AgentAssetDeclaredState::Unknown,
                trusted,
            },
            facts: BTreeMap::new(),
        },
    );
    asset.native_payload = payload(request, evidence);
    asset
}

pub(in crate::services::agent_cli::grok::environment) fn parse_source(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) -> bool {
    if !request.source.native_source_key.starts_with(SOURCE_PREFIX) {
        return false;
    }
    let Some(key) = SourceKey::decode(&request.source.native_source_key) else {
        malformed(output, AgentAssetDocumentFormat::Manifest);
        return true;
    };
    match &key.kind {
        SourceKind::Package => {
            if matches!(
                request.snapshot,
                AgentAssetSnapshot::DirectoryManifest { complete: true, .. }
            ) {
                let _ = output.emit_declaration(probe(
                    request,
                    Evidence::PackageDirectory,
                    "package-directory",
                ));
            }
        }
        SourceKind::Manifest { .. } => match request.snapshot {
            AgentAssetSnapshot::Missing { .. } => {
                let _ = output.emit_declaration(probe(
                    request,
                    Evidence::MissingManifest,
                    "manifest-missing",
                ));
            }
            AgentAssetSnapshot::File { bytes, .. } => {
                let value = config_document::parse(bytes, ConfigDocumentFormat::Json);
                let descriptor = value.as_ref().and_then(manifest::decode);
                if let (Some(value), Some(descriptor)) = (value, descriptor) {
                    let Some(parent) =
                        parent_from_package(&key, revision(request.snapshot), &descriptor)
                    else {
                        malformed(output, AgentAssetDocumentFormat::Manifest);
                        return true;
                    };
                    incomplete(
                        output,
                        AgentAssetCategory::Plugin,
                        AgentAssetDiscoveryIncompleteReason::RuntimeStateUnobserved,
                    );
                    let raw = Arc::new(value);
                    if output
                        .emit_declaration(package(
                            request,
                            &descriptor,
                            Evidence::Manifest(raw.as_ref().clone()),
                        ))
                        .is_break()
                    {
                        return true;
                    }
                    if descriptor.mcp_inline.is_some() {
                        parse_mcp_document(request, &parent, Arc::clone(&raw), true, output);
                    } else if manifest::component_path(
                        &request.source.allowed_root,
                        &descriptor.mcp_path,
                    ) == key.relative_path()
                    {
                        parse_mcp_document(request, &parent, Arc::clone(&raw), false, output);
                    }
                    if descriptor.hooks_inline.is_some() {
                        parse_hook_document(
                            request,
                            &parent,
                            raw,
                            descriptor.hooks_inline.as_ref(),
                            output,
                        );
                    } else if descriptor.hooks_path.as_deref().and_then(|path| {
                        manifest::component_path(&request.source.allowed_root, path)
                    }) == key.relative_path()
                    {
                        parse_hook_document(request, &parent, raw, None, output);
                    }
                } else {
                    malformed(output, AgentAssetDocumentFormat::Manifest);
                    incomplete(
                        output,
                        AgentAssetCategory::Hook,
                        AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
                    );
                }
            }
            _ => {}
        },
        SourceKind::Convention { component, .. } => {
            let hook_source = CONVENTIONS
                .get(usize::from(*component))
                .is_some_and(|(path, _)| *path == "hooks/hooks.json");
            if hook_source && matches!(request.snapshot, AgentAssetSnapshot::Blocked { .. }) {
                incomplete(
                    output,
                    AgentAssetCategory::Hook,
                    AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
                );
            }
            let exists = CONVENTIONS
                .get(usize::from(*component))
                .is_some_and(|(_, kind)| {
                    matches!(
                        (request.snapshot, kind),
                        (AgentAssetSnapshot::File { .. }, AgentAssetSourceKind::File)
                            | (
                                AgentAssetSnapshot::DirectoryManifest { complete: true, .. },
                                AgentAssetSourceKind::Directory
                            )
                    )
                });
            if exists {
                if let Some(descriptor) = manifest::convention(&key.package.directory) {
                    let Some(parent) =
                        parent_from_package(&key, revision(request.snapshot), &descriptor)
                    else {
                        malformed(output, AgentAssetDocumentFormat::Manifest);
                        return true;
                    };
                    incomplete(
                        output,
                        AgentAssetCategory::Plugin,
                        AgentAssetDiscoveryIncompleteReason::RuntimeStateUnobserved,
                    );
                    let colocated_mcp = request.source.source_kind == AgentAssetSourceKind::File
                        && manifest::component_path(
                            &request.source.allowed_root,
                            &descriptor.mcp_path,
                        ) == key.relative_path();
                    let raw = if colocated_mcp || hook_source {
                        component_document(request, hook_source, output)
                    } else {
                        None
                    };
                    if output
                        .emit_declaration(package(
                            request,
                            &descriptor,
                            Evidence::Convention(raw.clone()),
                        ))
                        .is_break()
                    {
                        return true;
                    }
                    if let Some(raw) = raw {
                        if colocated_mcp {
                            parse_mcp_document(request, &parent, Arc::clone(&raw), false, output);
                        }
                        if hook_source {
                            parse_hook_document(request, &parent, raw, None, output);
                        }
                    }
                }
            }
        }
        SourceKind::SkillDirectory { .. } => {}
        SourceKind::ComponentFile {
            parent,
            path,
            roles,
        } => {
            if parse_skills(request, &key, parent, path, &roles.skills, output).is_break() {
                return true;
            }
            if roles.mcp || roles.hooks {
                if let Some(raw) = component_document(request, roles.hooks, output) {
                    if roles.mcp {
                        parse_mcp_document(request, parent, Arc::clone(&raw), false, output);
                    }
                    if roles.hooks {
                        parse_hook_document(request, parent, raw, None, output);
                    }
                }
            }
        }
    }
    true
}

fn parse_skills(
    request: AgentAssetParseRequest<'_>,
    key: &SourceKey,
    parent: &Parent,
    path: &Path,
    roles: &[SkillRole],
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<()> {
    if roles.is_empty() {
        return ControlFlow::Continue(());
    }
    let AgentAssetSnapshot::File { bytes, .. } = request.snapshot else {
        return ControlFlow::Continue(());
    };
    let Ok(text) = std::str::from_utf8(bytes) else {
        malformed(output, AgentAssetDocumentFormat::Manifest);
        return ControlFlow::Continue(());
    };
    for role in roles {
        let basename = role.basename(path, &key.package.directory);
        let Some((bare, label, invocation_policy)) = manifest::skill(text, basename) else {
            malformed(output, AgentAssetDocumentFormat::Manifest);
            continue;
        };
        let native_id = format!("{}:{bare}", parent.namespace);
        let reference = parent.reference();
        let mut asset = parsed_asset(
            request,
            ParsedAssetInput {
                declaration_key: &role.declaration_key(),
                resolution_group_key: &bare,
                category: AgentAssetCategory::Skill,
                native_id: &native_id,
                label: &label,
                logical_origin: physical_origin(request.source),
                declared_state: AgentAssetDeclaredState::Unknown,
                trust_state: request.context.trust_context,
                role: AgentAssetDeclarationRole::Definition,
                participation: AgentAssetResolutionParticipation::Participates,
                provided_by: Some(reference.clone()),
                action_owner: Some(reference),
                explicitly_affected: Vec::new(),
                details: AgentAssetDetails::Skill {
                    enabled: AgentAssetDeclaredState::Unknown,
                    invocation_policy,
                },
                facts: BTreeMap::new(),
            },
        );
        asset.native_payload = payload(request, Evidence::Skill(text.to_owned()));
        if output.emit_declaration(asset).is_break() {
            return ControlFlow::Break(());
        }
    }
    ControlFlow::Continue(())
}

pub(super) fn parent_from_package(
    key: &SourceKey,
    revision: &str,
    descriptor: &manifest::Descriptor,
) -> Option<Parent> {
    let (evidence, package_revision) = match &key.kind {
        SourceKind::Manifest {
            rank,
            package_revision,
        } => (ParentEvidence::Manifest(*rank), package_revision),
        SourceKind::Convention {
            component,
            package_revision,
        } => (ParentEvidence::Convention(*component), package_revision),
        _ => return None,
    };
    Some(Parent {
        package: key.package.clone(),
        package_revision: package_revision.clone(),
        evidence,
        revision: revision.to_owned(),
        namespace: descriptor.namespace.clone(),
    })
}

fn component_document(
    request: AgentAssetParseRequest<'_>,
    hook_source: bool,
    output: &mut dyn AgentParseOutput,
) -> Option<Arc<serde_json::Value>> {
    let AgentAssetSnapshot::File { bytes, .. } = request.snapshot else {
        if hook_source && !matches!(request.snapshot, AgentAssetSnapshot::Missing { .. }) {
            incomplete(
                output,
                AgentAssetCategory::Hook,
                AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            );
        }
        return None;
    };
    let raw = if hook_source {
        config_document::parse(bytes, ConfigDocumentFormat::Json)
    } else {
        parse_json(bytes, Some(AgentAssetCategory::Mcp), output).ok()
    };
    let Some(raw) = raw else {
        malformed(output, AgentAssetDocumentFormat::Json);
        if hook_source {
            incomplete(
                output,
                AgentAssetCategory::Hook,
                AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            );
        }
        return None;
    };
    Some(Arc::new(raw))
}

fn parse_hook_document(
    request: AgentAssetParseRequest<'_>,
    parent: &Parent,
    raw: Arc<serde_json::Value>,
    inline_value: Option<&serde_json::Value>,
    output: &mut dyn AgentParseOutput,
) {
    let inline = inline_value.is_some();
    let value = inline_value.unwrap_or(raw.as_ref());
    // Native parse_hook_file returns zero definitions when the top-level value
    // has no hooks key, including a scalar inline value.
    let Some(root) = value.as_object() else {
        return;
    };
    let Some(namespace) = manifest::hook_namespace(&parent.namespace, &request.source.path, inline)
    else {
        incomplete(
            output,
            AgentAssetCategory::Hook,
            AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
        );
        return;
    };
    let complete = super::super::hooks::walk_slots(root, true, |event, group, handler| {
        let native_id = super::super::hooks::native_id(&namespace, event, group, handler);
        let reference = parent.reference();
        let mut asset = parsed_asset(
            request,
            ParsedAssetInput {
                declaration_key: &native_id,
                resolution_group_key: &native_id,
                category: AgentAssetCategory::Hook,
                native_id: &native_id,
                label: &format!("Grok Hook：{event}"),
                logical_origin: physical_origin(request.source),
                declared_state: AgentAssetDeclaredState::Enabled,
                trust_state: package_trust(request.source.scope, request.context.trust_context),
                role: AgentAssetDeclarationRole::Definition,
                participation: AgentAssetResolutionParticipation::Participates,
                provided_by: Some(reference.clone()),
                action_owner: Some(reference),
                explicitly_affected: Vec::new(),
                details: AgentAssetDetails::Hook {
                    managed: false,
                    enabled: AgentAssetDeclaredState::Enabled,
                    rule_count: Some(1),
                },
                facts: BTreeMap::new(),
            },
        );
        asset.native_payload = payload(
            request,
            Evidence::Hook {
                raw: Arc::clone(&raw),
                event: event.to_owned(),
                group,
                handler,
            },
        );
        output.emit_declaration(asset).is_continue()
    });
    if !complete {
        malformed(output, AgentAssetDocumentFormat::Json);
        incomplete(
            output,
            AgentAssetCategory::Hook,
            AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
        );
    }
}

fn parse_mcp_document(
    request: AgentAssetParseRequest<'_>,
    parent: &Parent,
    raw: Arc<serde_json::Value>,
    inline: bool,
    output: &mut dyn AgentParseOutput,
) {
    let inline_value = if inline {
        manifest::decode(&raw).and_then(|manifest| manifest.mcp_inline)
    } else {
        None
    };
    let value = if inline {
        if parent.revision != revision(request.snapshot) {
            return;
        }
        let Some(value) = inline_value.as_ref() else {
            return;
        };
        value
    } else {
        raw.as_ref()
    };
    let Some(entries) = manifest::mcp_entries(value, inline) else {
        // File MCP requires the native wrapper; only inline accepts bare maps.
        malformed(output, AgentAssetDocumentFormat::Json);
        return;
    };
    if inline
        && entries
            .values()
            .any(|entry| manifest::mcp_transport(entry).is_none())
    {
        malformed(output, AgentAssetDocumentFormat::Json);
        return;
    }
    for (native_id, entry) in bounded_object_entries(entries) {
        let Some(transport) = manifest::mcp_transport(entry) else {
            malformed(output, AgentAssetDocumentFormat::Json);
            continue;
        };
        if native_id.trim().is_empty() {
            continue;
        }
        if manifest::runtime_unobserved(entry) {
            incomplete(
                output,
                AgentAssetCategory::Mcp,
                AgentAssetDiscoveryIncompleteReason::RuntimeStateUnobserved,
            );
        }
        let state = entry
            .get("enabled")
            .and_then(serde_json::Value::as_bool)
            .map_or(AgentAssetDeclaredState::Unknown, |enabled| {
                if enabled {
                    AgentAssetDeclaredState::Enabled
                } else {
                    AgentAssetDeclaredState::Disabled
                }
            });
        let reference = parent.reference();
        let mut asset = parsed_asset(
            request,
            ParsedAssetInput {
                declaration_key: &format!("mcpServers:{native_id}"),
                resolution_group_key: native_id,
                category: AgentAssetCategory::Mcp,
                native_id,
                label: native_id,
                logical_origin: physical_origin(request.source),
                declared_state: state,
                trust_state: package_trust(request.source.scope, request.context.trust_context),
                role: AgentAssetDeclarationRole::Definition,
                participation: AgentAssetResolutionParticipation::Participates,
                provided_by: Some(reference.clone()),
                action_owner: Some(reference),
                explicitly_affected: Vec::new(),
                details: AgentAssetDetails::Mcp {
                    transport,
                    declared_state: state,
                    approval_state: AgentMcpApprovalState::NotRequired,
                    effective_availability: availability_from_declared(state),
                },
                facts: BTreeMap::new(),
            },
        );
        asset.native_payload = payload(request, Evidence::Mcp(Arc::clone(&raw)));
        if output.emit_declaration(asset).is_break() {
            return;
        }
    }
}
