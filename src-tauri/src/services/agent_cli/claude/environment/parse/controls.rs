use super::super::policy::decode_policy_rules;
use super::*;
use crate::models::{AgentAssetDeclarationRole, AgentAssetResolutionParticipation};

fn details(category: AgentAssetCategory, state: AgentAssetDeclaredState) -> AgentAssetDetails {
    match category {
        AgentAssetCategory::Mcp => AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Unknown,
            declared_state: state,
            approval_state: AgentMcpApprovalState::Unknown,
            effective_availability: AgentAssetEffectiveAvailability::Unknown,
        },
        AgentAssetCategory::Plugin => AgentAssetDetails::Plugin {
            install_state: crate::models::AgentAssetInstallState::Unknown,
            enabled: state,
            trusted: AgentTrustState::Unknown,
        },
        AgentAssetCategory::Hook => AgentAssetDetails::Hook {
            managed: false,
            enabled: state,
            rule_count: Some(0),
        },
        AgentAssetCategory::StatusUi => AgentAssetDetails::StatusUi {
            mode: crate::models::AgentStatusUiMode::Unknown,
            command_present: false,
        },
        AgentAssetCategory::Skill | AgentAssetCategory::Extension => {
            unreachable!("no Claude control family")
        }
    }
}

struct ControlInput<'a> {
    category: AgentAssetCategory,
    native_id: &'a str,
    key: &'a str,
    state: AgentAssetDeclaredState,
    role: AgentAssetDeclarationRole,
    origin: AgentAssetLogicalOrigin,
    participation: AgentAssetResolutionParticipation,
    payload: ClaudeControlPayload,
}

fn emit(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
    input: ControlInput<'_>,
) -> ControlFlow<AgentOutputStop> {
    let mut declaration = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: input.key,
            resolution_group_key: input.native_id,
            category: input.category,
            native_id: input.native_id,
            label: input.native_id,
            logical_origin: input.origin,
            declared_state: input.state,
            trust_state: declaration_trust_for_participation(
                request.context.trust_context,
                input.participation,
            ),
            role: input.role,
            participation: input.participation,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: details(input.category, input.state),
            facts: BTreeMap::new(),
        },
    );
    declaration.native_payload = AgentAssetNativePayload::ClaudeControl(input.payload);
    output.emit_declaration(declaration)
}

fn root(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
    category: AgentAssetCategory,
    field: &str,
    payload: ClaudeControlPayload,
    origin: AgentAssetLogicalOrigin,
    participation: AgentAssetResolutionParticipation,
) -> ControlFlow<AgentOutputStop> {
    let id = format!("control:{}:{field}", category.key());
    emit(
        request,
        output,
        ControlInput {
            category,
            native_id: &id,
            key: &id,
            state: AgentAssetDeclaredState::Unknown,
            role: match &payload {
                ClaudeControlPayload::ListRoot { family, .. } => family.role(),
                _ => source_role(category),
            },
            origin,
            participation,
            payload,
        },
    )
}

fn parse_policy_list(
    request: AgentAssetParseRequest<'_>,
    value: &Map<String, Value>,
    family: ListFamily,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let decoded = decode_policy_rules(value, family);
    if matches!(
        decoded.state,
        ListState::Invalid | ListState::NativeFailClosed
    ) {
        emit_malformed(output, family.field());
    }
    let count = decoded.entries.len();
    for (ordinal, matcher) in decoded.entries.into_iter().enumerate() {
        let safe_id = format!("control:{}:{ordinal}", family.field());
        let native_id = match &matcher {
            Some(PolicyMatcher::Name(name)) => name.as_str(),
            _ => safe_id.as_str(),
        };
        let key = format!("policy:{}:{ordinal}:{native_id}", family.field());
        let state = if matcher.is_none() {
            AgentAssetDeclaredState::Unknown
        } else if family == ListFamily::Allowed {
            AgentAssetDeclaredState::Enabled
        } else {
            AgentAssetDeclaredState::Rejected
        };
        emit(
            request,
            output,
            ControlInput {
                category: AgentAssetCategory::Mcp,
                native_id,
                key: &key,
                state,
                role: AgentAssetDeclarationRole::PolicyOverlay,
                origin: physical_origin(request.source),
                participation: suppressed(request),
                payload: ClaudeControlPayload::RuleEntry {
                    family,
                    ordinal,
                    matcher: matcher.clone(),
                },
            },
        )?;
    }
    root(
        request,
        output,
        AgentAssetCategory::Mcp,
        &format!("{}:root", family.field()),
        ClaudeControlPayload::ListRoot {
            family,
            state: decoded.state,
            expected_entry_count: count,
        },
        physical_origin(request.source),
        suppressed(request),
    )
}

fn parse_names(
    request: AgentAssetParseRequest<'_>,
    value: Option<&Value>,
    family: ListFamily,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let (entries, state) = match value {
        None => (Vec::new(), ListState::Absent),
        Some(Value::Array(values)) => {
            let entries = values
                .iter()
                .map(|value| value.as_str().map(str::to_owned))
                .collect::<Vec<_>>();
            let state = if entries.iter().all(Option::is_some) {
                ListState::Valid
            } else {
                ListState::Invalid
            };
            (entries, state)
        }
        Some(_) => (Vec::new(), ListState::Invalid),
    };
    if state == ListState::Invalid {
        emit_malformed(output, family.field());
    }
    let origin = if family.personal() {
        AgentAssetLogicalOrigin {
            scope: AgentAssetScope::Local,
            precedence: 30,
        }
    } else {
        physical_origin(request.source)
    };
    let participation =
        if family.personal() && request.context.trust_context != AgentTrustState::Trusted {
            AgentAssetResolutionParticipation::Suppressed {
                reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
            }
        } else {
            suppressed(request)
        };
    let count = entries.len();
    for (ordinal, name) in entries.into_iter().enumerate() {
        let safe_id = format!("control:{}:{ordinal}", family.field());
        let native_id = name.as_deref().unwrap_or(&safe_id);
        let prefix = if family.personal() {
            "state"
        } else {
            "approval"
        };
        let key = format!("{prefix}:{}:{ordinal}:{native_id}", family.field());
        let state = if name.is_none() {
            AgentAssetDeclaredState::Unknown
        } else {
            match family {
                ListFamily::PersonalDisabled => AgentAssetDeclaredState::Disabled,
                ListFamily::Rejected => AgentAssetDeclaredState::Rejected,
                _ => AgentAssetDeclaredState::Enabled,
            }
        };
        emit(
            request,
            output,
            ControlInput {
                category: AgentAssetCategory::Mcp,
                native_id,
                key: &key,
                state,
                role: family.role(),
                origin,
                participation,
                payload: ClaudeControlPayload::NameEntry {
                    family,
                    ordinal,
                    name: name.clone(),
                },
            },
        )?;
    }
    root(
        request,
        output,
        AgentAssetCategory::Mcp,
        &format!("{}:root", family.field()),
        ClaudeControlPayload::ListRoot {
            family,
            state,
            expected_entry_count: count,
        },
        origin,
        participation,
    )
}

fn parse_boolean(
    request: AgentAssetParseRequest<'_>,
    value: &Map<String, Value>,
    category: AgentAssetCategory,
    field: BooleanField,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let value = match value.get(field.field()) {
        None => BooleanValue::Absent,
        Some(Value::Bool(value)) => BooleanValue::Bool(*value),
        Some(_) if field.managed() => BooleanValue::NativeFailClosed,
        Some(_) => BooleanValue::Invalid,
    };
    // One source field can produce separate Hook and StatusUi witnesses.
    // Diagnose its schema once while preserving both typed declarations.
    let diagnostic_owner = field != BooleanField::DisableAllHooks
        || category == AgentAssetCategory::Hook
        || !request
            .source
            .categories
            .contains(&AgentAssetCategory::Hook);
    if diagnostic_owner
        && matches!(
            value,
            BooleanValue::Invalid | BooleanValue::NativeFailClosed
        )
    {
        emit_malformed(output, field.field());
    }
    root(
        request,
        output,
        category,
        field.field(),
        ClaudeControlPayload::Boolean { field, value },
        physical_origin(request.source),
        suppressed(request),
    )
}

fn parse_plugins(
    request: AgentAssetParseRequest<'_>,
    value: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let (plugins, state) = match value.get("enabledPlugins") {
        None => (None, ListState::Absent),
        Some(Value::Object(plugins)) => (Some(plugins), ListState::Valid),
        Some(_) => {
            emit_malformed(output, "enabledPlugins");
            (None, ListState::Invalid)
        }
    };
    let count = plugins.map_or(0, Map::len);
    if let Some(plugins) = plugins {
        for (native_id, value) in bounded_object_entries(plugins) {
            let enabled = value.as_bool();
            let state = match enabled {
                Some(true) => AgentAssetDeclaredState::Enabled,
                Some(false) => AgentAssetDeclaredState::Disabled,
                None => AgentAssetDeclaredState::Unknown,
            };
            emit(
                request,
                output,
                ControlInput {
                    category: AgentAssetCategory::Plugin,
                    native_id,
                    key: &format!("state:{native_id}"),
                    state,
                    role: AgentAssetDeclarationRole::StateOverlay,
                    origin: physical_origin(request.source),
                    participation: suppressed(request),
                    payload: ClaudeControlPayload::PluginEntry { enabled },
                },
            )?;
        }
    }
    root(
        request,
        output,
        AgentAssetCategory::Plugin,
        "enabledPlugins:root",
        ClaudeControlPayload::PluginRoot {
            state,
            expected_entry_count: count,
        },
        physical_origin(request.source),
        suppressed(request),
    )
}

pub(super) fn parse_account_controls(
    request: AgentAssetParseRequest<'_>,
    value: Option<&Map<String, Value>>,
    state: SourceState,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    if let Some(value) = value {
        let project = request
            .context
            .workspace_id
            .as_deref()
            .and_then(|workspace| {
                let projects = value.get("projects")?.as_object()?;
                match select_exact_project(
                    projects,
                    Path::new(workspace),
                    request.workspace_lexical,
                ) {
                    ProjectSelection::Unique(project) => Some(project),
                    _ => None,
                }
            });
        for family in [ListFamily::PersonalDisabled, ListFamily::PersonalEnabled] {
            parse_names(
                request,
                project.and_then(|project| project.get(family.field())),
                family,
                output,
            )?;
        }
    }
    root(
        request,
        output,
        AgentAssetCategory::Mcp,
        "source",
        ClaudeControlPayload::SourceRoot { state },
        physical_origin(request.source),
        suppressed(request),
    )
}

pub(super) fn parse_control_sources(
    request: AgentAssetParseRequest<'_>,
    value: Option<&Map<String, Value>>,
    state: SourceState,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let key = request.source.native_source_key.as_str();
    if key == "account" {
        return parse_account_controls(request, value, state, output);
    }
    if key == "managed-mcp" {
        if let Some(value) = value {
            let state = match value.get("mcpServers") {
                None => NamespaceState::Absent,
                Some(Value::Object(servers)) => {
                    NamespaceState::Object(servers.keys().cloned().collect())
                }
                Some(_) => NamespaceState::InvalidContainer,
            };
            root(
                request,
                output,
                AgentAssetCategory::Mcp,
                "managed-namespace",
                ClaudeControlPayload::ManagedNamespace { state },
                physical_origin(request.source),
                suppressed(request),
            )?;
        }
        return root(
            request,
            output,
            AgentAssetCategory::Mcp,
            "source",
            ClaudeControlPayload::SourceRoot { state },
            physical_origin(request.source),
            suppressed(request),
        );
    }
    if !SETTINGS_AUTHORITIES.contains(&key) {
        return ControlFlow::Continue(());
    }
    for category in [
        AgentAssetCategory::Mcp,
        AgentAssetCategory::Plugin,
        AgentAssetCategory::Hook,
        AgentAssetCategory::StatusUi,
    ] {
        if !request.source.categories.contains(&category) {
            continue;
        }
        if let Some(value) = value {
            match category {
                AgentAssetCategory::Mcp => {
                    for family in [ListFamily::Approved, ListFamily::Rejected] {
                        parse_names(request, value.get(family.field()), family, output)?;
                    }
                    for family in [ListFamily::Allowed, ListFamily::Denied] {
                        parse_policy_list(request, value, family, output)?;
                    }
                    parse_boolean(request, value, category, BooleanField::ApproveAll, output)?;
                    if key == "managed-settings" {
                        parse_boolean(
                            request,
                            value,
                            category,
                            BooleanField::ManagedMcpOnly,
                            output,
                        )?;
                    }
                }
                AgentAssetCategory::Plugin => {
                    parse_plugins(request, value, output)?;
                }
                AgentAssetCategory::Hook | AgentAssetCategory::StatusUi => {
                    parse_boolean(
                        request,
                        value,
                        category,
                        BooleanField::DisableAllHooks,
                        output,
                    )?;
                    if category == AgentAssetCategory::Hook && key == "managed-settings" {
                        parse_boolean(
                            request,
                            value,
                            category,
                            BooleanField::ManagedHooksOnly,
                            output,
                        )?;
                    }
                }
                AgentAssetCategory::Skill | AgentAssetCategory::Extension => {
                    unreachable!("fixed category list")
                }
            }
        }
        root(
            request,
            output,
            category,
            "source",
            ClaudeControlPayload::SourceRoot { state },
            physical_origin(request.source),
            suppressed(request),
        )?;
    }
    ControlFlow::Continue(())
}
