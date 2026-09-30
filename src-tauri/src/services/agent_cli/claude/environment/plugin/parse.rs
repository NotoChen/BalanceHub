use super::*;
use crate::models::{
    AgentAssetDeclarationRole, AgentAssetDeclaredState, AgentAssetDetails,
    AgentAssetEffectiveAvailability, AgentAssetInstallState, AgentAssetPresence,
    AgentAssetResolutionParticipation, AgentMcpApprovalState, AgentMcpTransport, AgentTrustState,
};
use crate::services::agent_cli::contracts::{
    AgentAssetNativePayload, AgentAssetParseRequest, AgentOutputStop, AgentParseOutput,
};
use crate::services::agent_cli::environment::{
    availability_from_declared, parsed_asset, ParsedAssetInput,
};
use std::{collections::BTreeMap, ops::ControlFlow};

pub(in crate::services::agent_cli::claude::environment) fn parse_source(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) -> bool {
    if !request.source.native_source_key.starts_with(PREFIX) {
        return false;
    }
    let Some(bindings) = source_bindings(request.source) else {
        malformed(output, "sourceBinding");
        return true;
    };
    for binding in &bindings {
        if parse_binding(request, binding, output).is_break() {
            break;
        }
    }
    true
}

fn parse_binding(
    request: AgentAssetParseRequest<'_>,
    binding: &SourceBinding,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let key = &binding.key;
    match &binding.role {
        SourceRole::Root => {
            let entries = match request.snapshot {
                AgentAssetSnapshot::DirectoryManifest {
                    entries,
                    complete: true,
                    ..
                } => Some(entries.clone()),
                _ => None,
            };
            emit_metadata(
                request,
                binding,
                "root",
                ClaudePluginPayload::Root {
                    key: key.clone(),
                    entries,
                    missing: matches!(request.snapshot, AgentAssetSnapshot::Missing { .. }),
                },
                output,
            )?;
        }
        SourceRole::Manifest => {
            let parsed = json(request.snapshot, output);
            let value = match request.snapshot {
                AgentAssetSnapshot::Missing { .. } => decode_manifest(None, key),
                AgentAssetSnapshot::File { .. } => parsed
                    .as_ref()
                    .and_then(|value| decode_manifest(Some(value), key)),
                _ => None,
            };
            if value.is_none() {
                malformed(output, "manifest");
                unobserved(output, "hooks.manifest");
            }
            emit_metadata(
                request,
                binding,
                "manifest",
                ClaudePluginPayload::Manifest {
                    key: key.clone(),
                    value: value.clone(),
                },
                output,
            )?;
            if let Some(manifest) = value {
                for (ordinal, component) in manifest.hooks.iter().enumerate() {
                    match component {
                        hooks::HookComponent::Inline(document) => hooks::emit_document(
                            request,
                            binding,
                            &manifest.namespace,
                            ordinal + 1,
                            Some(document.clone()),
                            output,
                        )?,
                        hooks::HookComponent::Invalid => {
                            unobserved(output, "hooks.invalidComponent");
                            hooks::emit(
                                request,
                                binding,
                                &manifest.namespace,
                                ordinal + 1,
                                Some(&Value::Null),
                                output,
                            )?;
                        }
                        hooks::HookComponent::File(relative) => {
                            if physical_key(
                                &request.source.allowed_root.join(relative),
                                &request.source.allowed_root,
                                AgentAssetSourceKind::File,
                            ) == physical_key(
                                &request.source.path,
                                &request.source.allowed_root,
                                AgentAssetSourceKind::File,
                            ) {
                                unobserved(output, "hooks.manifestSelfReference");
                                hooks::emit(
                                    request,
                                    binding,
                                    &manifest.namespace,
                                    ordinal + 1,
                                    parsed.as_ref(),
                                    output,
                                )?;
                            }
                        }
                    }
                }
                for field in &manifest.unsupported {
                    unobserved(output, field);
                }
                for (index, component) in manifest.mcp.iter().enumerate() {
                    match component {
                        McpComponent::Invalid => malformed(output, "mcpServers.component"),
                        McpComponent::Inline(servers) => {
                            emit_servers(
                                request,
                                binding,
                                &manifest.namespace,
                                index + 1,
                                servers,
                                output,
                            )?;
                        }
                        McpComponent::File(relative)
                            if physical_key(
                                &request.source.allowed_root.join(relative),
                                &request.source.allowed_root,
                                AgentAssetSourceKind::File,
                            ) == physical_key(
                                &request.source.path,
                                &request.source.allowed_root,
                                AgentAssetSourceKind::File,
                            ) =>
                        {
                            if let Some(root) = parsed.as_ref().and_then(Value::as_object) {
                                if let Some(servers) = mcp_servers(root, output) {
                                    emit_servers(
                                        request,
                                        binding,
                                        &manifest.namespace,
                                        index + 1,
                                        servers,
                                        output,
                                    )?;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        SourceRole::Skill {
            namespace,
            root_index,
            mode,
        } => {
            parse_skill(request, binding, namespace, *root_index, mode, output)?;
        }
        SourceRole::Hook { namespace, ordinal } => {
            let value = json(request.snapshot, output);
            hooks::emit(
                request,
                binding,
                namespace,
                *ordinal,
                value.as_ref(),
                output,
            )?;
        }
        SourceRole::Mcp { namespace, ordinal } => {
            if let Some(value) = json(request.snapshot, output) {
                if let Some(root) = value.as_object() {
                    if let Some(servers) = mcp_servers(root, output) {
                        emit_servers(request, binding, namespace, *ordinal, servers, output)?;
                    }
                } else {
                    malformed(output, "mcpServers");
                }
            }
        }
        SourceRole::Directory => {}
    }
    ControlFlow::Continue(())
}

fn mcp_servers<'a>(
    root: &'a Map<String, Value>,
    output: &mut dyn AgentDiagnosticOutput,
) -> Option<&'a Map<String, Value>> {
    match root.get("mcpServers") {
        Some(Value::Object(servers)) => Some(servers),
        Some(_) => {
            malformed(output, "mcpServers");
            None
        }
        None => Some(root),
    }
}

fn emit_metadata(
    request: AgentAssetParseRequest<'_>,
    binding: &SourceBinding,
    kind: &str,
    payload: ClaudePluginPayload,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let key = &binding.key;
    let mut asset = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: &format!("package.{kind}:{}", key.occurrence),
            resolution_group_key: &key.id,
            category: AgentAssetCategory::Plugin,
            native_id: &key.id,
            label: &key.id,
            logical_origin: binding.origin(),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: AgentTrustState::Unknown,
            role: AgentAssetDeclarationRole::StateOverlay,
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
    asset.presence = match request.snapshot {
        AgentAssetSnapshot::Missing { .. } => AgentAssetPresence::Missing,
        AgentAssetSnapshot::Blocked { .. } => AgentAssetPresence::Blocked,
        AgentAssetSnapshot::File { .. } | AgentAssetSnapshot::DirectoryManifest { .. } => {
            AgentAssetPresence::Present
        }
    };
    asset.native_payload = AgentAssetNativePayload::ClaudePlugin(payload);
    output.emit_declaration(asset)
}

pub(in crate::services::agent_cli::claude::environment) fn skill_name(
    namespace: &str,
    path: &Path,
    mode: &str,
    frontmatter_name: Option<&str>,
) -> Option<String> {
    let fallback = if mode.starts_with("command") {
        path.file_stem()?.to_str()?
    } else {
        path.parent()?.file_name()?.to_str()?
    };
    let name = if mode == "root" {
        let value = frontmatter_name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(fallback);
        value
            .strip_prefix(&format!("{namespace}:"))
            .unwrap_or(value)
    } else {
        fallback
    };
    let name = normalize_skill(name);
    (!name.is_empty()).then(|| format!("{namespace}:{name}"))
}

fn parse_skill(
    request: AgentAssetParseRequest<'_>,
    binding: &SourceBinding,
    namespace: &str,
    root_index: usize,
    mode: &str,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let key = &binding.key;
    let AgentAssetSnapshot::File { bytes, .. } = request.snapshot else {
        return ControlFlow::Continue(());
    };
    let metadata = super::super::parse::skill::decode_skill_frontmatter(bytes, output);
    let valid = metadata.is_some();
    let policy = metadata.as_ref().map_or(
        crate::models::AgentSkillInvocationPolicy::Unknown,
        |value| value.invocation_policy,
    );
    let label = metadata.and_then(|value| value.name);
    let Some(name) = skill_name(namespace, &request.source.path, mode, label.as_deref()) else {
        return ControlFlow::Continue(());
    };
    let state = if valid {
        AgentAssetDeclaredState::Enabled
    } else {
        AgentAssetDeclaredState::Unknown
    };
    let parent = parent_ref(key);
    let mut asset = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: &format!("plugin.skill:{}:{root_index}:{mode}", key.occurrence),
            resolution_group_key: &name,
            category: AgentAssetCategory::Skill,
            native_id: &name,
            label: &name,
            logical_origin: binding.origin(),
            declared_state: state,
            trust_state: request.context.trust_context,
            role: AgentAssetDeclarationRole::Definition,
            participation: AgentAssetResolutionParticipation::Participates,
            provided_by: Some(parent.clone()),
            action_owner: Some(parent),
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Skill {
                enabled: state,
                invocation_policy: policy,
            },
            facts: BTreeMap::new(),
        },
    );
    asset.native_payload = AgentAssetNativePayload::ClaudePlugin(ClaudePluginPayload::Skill {
        key: key.clone(),
        namespace: namespace.to_owned(),
        root_index,
        mode: mode.to_owned(),
        frontmatter_name: label,
        valid,
    });
    output.emit_declaration(asset)
}

pub(in crate::services::agent_cli::claude::environment) fn transport(
    value: &Value,
) -> AgentMcpTransport {
    let Some(object) = value.as_object() else {
        return AgentMcpTransport::Unknown;
    };
    if ["env", "headers"].iter().any(|field| {
        object.get(*field).is_some_and(|value| {
            value
                .as_object()
                .is_none_or(|values| values.values().any(|value| !value.is_string()))
        })
    }) {
        return AgentMcpTransport::Unknown;
    }
    if !object.contains_key("command") && !object.contains_key("type") {
        return AgentMcpTransport::Unknown;
    }
    let mut normalized = object.clone();
    if let Some(kind) = object.get("type").and_then(Value::as_str) {
        let kind = match kind {
            "ws" => "websocket",
            "streamable-http" => "http",
            kind => kind,
        };
        normalized.insert("type".into(), Value::String(kind.to_owned()));
    }
    super::super::parse::strict_transport(&Value::Object(normalized))
}

fn emit_servers(
    request: AgentAssetParseRequest<'_>,
    binding: &SourceBinding,
    namespace: &str,
    ordinal: usize,
    servers: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let key = &binding.key;
    for (name, value) in bounded_object_entries(servers) {
        let native_id = format!("plugin:{namespace}:{name}");
        let transport = transport(value);
        // Native loading rejects an invalid entry before merging components.
        // Keeping it as an Unknown definition would hide a valid earlier one.
        if transport == AgentMcpTransport::Unknown {
            malformed(output, "mcpServers.entry");
            continue;
        }
        let AgentAssetNativePayload::McpDefinition(definition) =
            super::super::parse::native_mcp_payload(value, transport)
        else {
            malformed(output, "mcpServers.entry");
            continue;
        };
        let unresolved = runtime_expansion(value);
        if unresolved {
            unobserved(output, "mcpServers.runtimeExpansion");
        }
        let state = if unresolved {
            AgentAssetDeclaredState::Unknown
        } else {
            AgentAssetDeclaredState::Enabled
        };
        let parent = parent_ref(key);
        let mut asset = parsed_asset(
            request,
            ParsedAssetInput {
                declaration_key: &format!("plugin.mcp:{}:{ordinal}:{name}", key.occurrence),
                resolution_group_key: &native_id,
                category: AgentAssetCategory::Mcp,
                native_id: &native_id,
                label: &native_id,
                logical_origin: binding.origin(),
                declared_state: state,
                trust_state: request.context.trust_context,
                role: AgentAssetDeclarationRole::Definition,
                participation: AgentAssetResolutionParticipation::Participates,
                provided_by: Some(parent.clone()),
                action_owner: Some(parent),
                explicitly_affected: Vec::new(),
                details: AgentAssetDetails::Mcp {
                    transport,
                    declared_state: state,
                    approval_state: if matches!(
                        binding.origin().scope,
                        AgentAssetScope::Workspace | AgentAssetScope::Local
                    ) {
                        AgentMcpApprovalState::Pending
                    } else {
                        AgentMcpApprovalState::NotRequired
                    },
                    effective_availability: if unresolved {
                        AgentAssetEffectiveAvailability::Unknown
                    } else {
                        availability_from_declared(state)
                    },
                },
                facts: BTreeMap::new(),
            },
        );
        asset.native_payload = AgentAssetNativePayload::ClaudePlugin(ClaudePluginPayload::Mcp {
            key: key.clone(),
            namespace: namespace.to_owned(),
            ordinal,
            value: value.clone(),
            definition,
        });
        output.emit_declaration(asset)?;
    }
    ControlFlow::Continue(())
}
