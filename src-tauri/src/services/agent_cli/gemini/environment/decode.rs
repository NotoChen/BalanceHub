use super::parse::{
    emit_invalid_hook_policy, parse_enablement, parse_extension_enablement, parse_extension_hooks,
    parse_extension_manifest, parse_settings, unknown_mcp_details,
};
use super::skill::parse_skill;
use super::*;
use crate::{
    models::*,
    services::agent_cli::{contracts::*, environment::*},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::ControlFlow,
};

pub(super) fn parse_assets(request: AgentAssetParseRequest<'_>, output: &mut dyn AgentParseOutput) {
    if request.source.source_kind == AgentAssetSourceKind::Directory {
        // Directories are only manifests for a fixed follow-up path. A
        // directory name alone is never sufficient evidence for an asset.
        return;
    }

    if request
        .source
        .native_source_key
        .starts_with(EXTENSION_MANIFEST_PREFIX)
    {
        parse_extension_manifest(request, output);
        return;
    }
    if request
        .source
        .native_source_key
        .starts_with(EXTENSION_HOOKS_PREFIX)
    {
        parse_extension_hooks(request, output);
        return;
    }
    if request
        .source
        .native_source_key
        .starts_with(EXTENSION_SKILL_PREFIX)
        || request
            .source
            .native_source_key
            .starts_with(SKILL_MANIFEST_PREFIX)
    {
        parse_skill(request, output);
        return;
    }

    if matches!(request.snapshot, AgentAssetSnapshot::Blocked { .. }) {
        let _ = emit_invalid_controls(request, output);
        return;
    }

    let AgentAssetSnapshot::File { bytes, .. } = request.snapshot else {
        return;
    };
    let is_jsonc = matches!(
        request.source.native_source_key.as_str(),
        "settings"
            | "workspace-settings"
            | "system-defaults"
            | "system-settings"
            | "trusted-folders"
    );
    let value = if is_jsonc {
        parse_gemini_jsonc(bytes, output, AgentAssetCategory::Mcp)
    } else {
        parse_strict_json(bytes, output, AgentAssetCategory::Mcp)
    };
    let Some(value) = value else {
        let _ = emit_invalid_controls(request, output);
        return;
    };
    let Some(root) = value.as_object() else {
        output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
            format: AgentAssetDocumentFormat::Json,
            location: None,
        });
        let _ = emit_invalid_controls(request, output);
        return;
    };
    if request.source.native_source_key == "mcp-enablement" {
        if parse_enablement(request, root, output).is_break() {
            return;
        }
        return;
    }
    if request.source.native_source_key == "extension-enablement" {
        parse_extension_enablement(request, root, output);
        return;
    }
    let _ = parse_settings(request, root, output);
}

fn is_mcp_control_source(request: AgentAssetParseRequest<'_>) -> bool {
    request.source.categories.contains(&AgentAssetCategory::Mcp)
        && matches!(
            request.source.native_source_key.as_str(),
            "settings" | "workspace-settings" | "system-defaults" | "system-settings"
        )
}

fn is_settings_control_source(request: AgentAssetParseRequest<'_>) -> bool {
    matches!(
        request.source.native_source_key.as_str(),
        "settings" | "workspace-settings" | "system-defaults" | "system-settings"
    )
}

fn emit_invalid_controls(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    if is_settings_control_source(request) {
        if is_mcp_control_source(request) {
            emit_invalid_mcp_controls(request, output)?;
        }
        if request
            .source
            .categories
            .contains(&AgentAssetCategory::Skill)
        {
            super::skill::emit_invalid_policy(request, output)?;
        }
        if request
            .source
            .categories
            .contains(&AgentAssetCategory::Hook)
        {
            super::parse::hook_incomplete(output);
            emit_invalid_hook_policy(request, output)?;
        }
        if request
            .source
            .categories
            .contains(&AgentAssetCategory::StatusUi)
        {
            emit_invalid_status_ui(request, output)?;
        }
    } else if request.source.native_source_key == "mcp-enablement" {
        emit_invalid_enablement(request, output, AgentAssetInvalidControl::McpEnablement)?;
    } else if request.source.native_source_key == "extension-enablement" {
        emit_invalid_enablement(
            request,
            output,
            AgentAssetInvalidControl::ExtensionEnablement,
        )?;
    }
    ControlFlow::Continue(())
}

fn emit_invalid_mcp_controls(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    for (id, payload) in [
        ("mcp.allowed", AgentMcpPolicyPayload::InvalidAllowed),
        ("mcp.excluded", AgentMcpPolicyPayload::InvalidExcluded),
    ] {
        if let ControlFlow::Break(reason) =
            super::parse::emit_mcp_policy_marker(request, output, id, payload)
        {
            return ControlFlow::Break(reason);
        }
    }
    ControlFlow::Continue(())
}

fn emit_invalid_enablement(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
    control: AgentAssetInvalidControl,
) -> ControlFlow<AgentOutputStop> {
    let category = match control {
        AgentAssetInvalidControl::McpEnablement => AgentAssetCategory::Mcp,
        AgentAssetInvalidControl::ExtensionEnablement => AgentAssetCategory::Extension,
    };
    let mut declaration = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: "invalid-control",
            resolution_group_key: request.source.native_source_key.as_str(),
            category,
            native_id: request.source.native_source_key.as_str(),
            label: request.source.native_source_key.as_str(),
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: request.context.trust_context,
            role: AgentAssetDeclarationRole::StateOverlay,
            participation: if category == AgentAssetCategory::Mcp {
                mcp_participation(request)
            } else {
                participation(request, category)
            },
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: if category == AgentAssetCategory::Mcp {
                unknown_mcp_details()
            } else {
                AgentAssetDetails::Extension {
                    install_state: AgentAssetInstallState::Installed,
                    enabled: AgentAssetDeclaredState::Unknown,
                    trusted: AgentTrustState::Unknown,
                }
            },
            facts: BTreeMap::new(),
        },
    );
    declaration.native_payload = AgentAssetNativePayload::InvalidControl(control);
    output.emit_declaration(declaration)
}

fn emit_invalid_status_ui(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    output.emit_declaration(parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: "footer.invalid-control",
            resolution_group_key: "footer",
            category: AgentAssetCategory::StatusUi,
            native_id: "footer",
            label: "Gemini CLI Status UI",
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: request.context.trust_context,
            role: AgentAssetDeclarationRole::Definition,
            participation: participation(request, AgentAssetCategory::StatusUi),
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::StatusUi {
                mode: AgentStatusUiMode::Unknown,
                command_present: false,
            },
            facts: BTreeMap::new(),
        },
    ))
}

pub(super) fn parse_strict_json(
    bytes: &[u8],
    output: &mut dyn AgentDiagnosticOutput,
    category: AgentAssetCategory,
) -> Option<Value> {
    if has_control_duplicate(bytes) {
        emit_malformed(output, "duplicate-control-key");
        return None;
    }
    match serde_json::from_slice(bytes) {
        Ok(value) => Some(value),
        Err(_) => {
            output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
                format: AgentAssetDocumentFormat::Json,
                location: None,
            });
            let _ = category;
            None
        }
    }
}

pub(super) fn parse_gemini_jsonc(
    bytes: &[u8],
    output: &mut dyn AgentDiagnosticOutput,
    category: AgentAssetCategory,
) -> Option<Value> {
    let mut stripped = Vec::with_capacity(bytes.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            stripped.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            stripped.push(byte);
            index += 1;
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                if bytes[index] == b'\n' {
                    stripped.push(b'\n');
                }
                index += 1;
            }
            index = (index + 2).min(bytes.len());
        } else {
            stripped.push(byte);
            index += 1;
        }
    }
    if has_control_duplicate(&stripped) {
        emit_malformed(output, "duplicate-control-key");
        return None;
    }
    parse_json(&stripped, Some(category), output).ok()
}

fn has_control_duplicate(bytes: &[u8]) -> bool {
    fn skip_ws(bytes: &[u8], index: &mut usize) {
        while bytes.get(*index).is_some_and(u8::is_ascii_whitespace) {
            *index += 1;
        }
    }
    fn string_end(bytes: &[u8], index: &mut usize) -> bool {
        if bytes.get(*index) != Some(&b'"') {
            return false;
        }
        *index += 1;
        let mut escaped = false;
        while let Some(byte) = bytes.get(*index) {
            *index += 1;
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                return true;
            }
        }
        false
    }
    fn value(bytes: &[u8], index: &mut usize, path: &mut Vec<String>) -> Option<bool> {
        skip_ws(bytes, index);
        match bytes.get(*index) {
            Some(b'{') => object(bytes, index, path),
            Some(b'[') => {
                *index += 1;
                loop {
                    skip_ws(bytes, index);
                    if bytes.get(*index) == Some(&b']') {
                        *index += 1;
                        return Some(false);
                    }
                    if value(bytes, index, path)? {
                        return Some(true);
                    }
                    skip_ws(bytes, index);
                    match bytes.get(*index) {
                        Some(b',') => *index += 1,
                        Some(b']') => {
                            *index += 1;
                            return Some(false);
                        }
                        _ => return None,
                    }
                }
            }
            Some(b'"') => string_end(bytes, index).then_some(false),
            Some(_) => {
                while let Some(byte) = bytes.get(*index) {
                    if matches!(byte, b',' | b']' | b'}') || byte.is_ascii_whitespace() {
                        break;
                    }
                    *index += 1;
                }
                Some(false)
            }
            None => None,
        }
    }
    fn object(bytes: &[u8], index: &mut usize, path: &mut Vec<String>) -> Option<bool> {
        *index += 1;
        let mut keys = BTreeSet::new();
        loop {
            skip_ws(bytes, index);
            if bytes.get(*index) == Some(&b'}') {
                *index += 1;
                return Some(false);
            }
            let start = *index;
            if !string_end(bytes, index) {
                return None;
            }
            let key = serde_json::from_slice::<String>(&bytes[start..*index]).ok()?;
            skip_ws(bytes, index);
            if bytes.get(*index) != Some(&b':') {
                return None;
            }
            *index += 1;
            // Gemini's JSONC control documents are authoritative overlays. A
            // duplicate at any depth is ambiguous after serde_json would
            // collapse it, so fail closed before materializing the map.
            let duplicate = !keys.insert(key.clone());
            path.push(key);
            let nested = value(bytes, index, path)?;
            path.pop();
            if duplicate || nested {
                return Some(true);
            }
            skip_ws(bytes, index);
            match bytes.get(*index) {
                Some(b',') => *index += 1,
                Some(b'}') => {
                    *index += 1;
                    return Some(false);
                }
                _ => return None,
            }
        }
    }
    let mut index = 0;
    let mut path = Vec::new();
    value(bytes, &mut index, &mut path).unwrap_or(true)
}

pub(super) fn emit_malformed(output: &mut dyn AgentDiagnosticOutput, location: &str) {
    output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
        format: AgentAssetDocumentFormat::Json,
        location: Some(location.to_owned()),
    });
}
