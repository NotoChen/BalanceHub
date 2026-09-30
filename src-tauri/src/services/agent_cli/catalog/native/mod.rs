//! Registry-owned native layouts. The catalog does not switch on Agent names.
#[cfg(test)]
pub(super) mod evidence;
pub(crate) mod hook_codec;
#[cfg(test)]
pub(crate) mod hook_evidence;
pub(crate) mod hooks;
mod mcp;
mod mcp_options;
mod mcp_variables;
mod removal;
pub(crate) use mcp_variables::validation_url;
#[cfg(test)]
mod tests;
use super::definition::McpDefinition;
use crate::models::*;
use crate::services::agent_cli::config_support::{parse_jsonc_document, rewrite_json_document};
use crate::services::agent_cli::environment::config_document::{self, ConfigDocumentFormat};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, path::Path};

pub(crate) struct NativeCatalogAdapter {
    pub key: &'static str,
    pub format: ConfigDocumentFormat,
    pub table: &'static str,
    pub file_name: &'static str,
    pub workspace_file_name: &'static str,
    pub headers: &'static str,
    pub http_url: &'static str,
    pub explicit_type: bool,
    pub supports_sse: bool,
    pub supports_websocket: bool,
    pub supports_cwd: bool,
    pub implicit_url_transport: AgentMcpTransport,
    /// Connection dependencies that cannot be reduced to command/URL/env/headers.
    connection_fields: &'static [&'static str],
    pub documentation: &'static str,
    pub config_directory: &'static str,
    pub shared_skills: bool,
    skill_requires_workspace_trust: bool,
    json_comments: bool,
    declaration_layout: McpDeclarationLayout,
}

enum McpDeclarationLayout {
    Root(&'static str),
    ClaudeScopes,
}

pub(crate) static CLAUDE: NativeCatalogAdapter = NativeCatalogAdapter {
    key: "claude",
    format: ConfigDocumentFormat::Json,
    table: "mcpServers",
    file_name: ".claude.json",
    workspace_file_name: ".mcp.json",
    headers: "headers",
    http_url: "url",
    explicit_type: true,
    supports_sse: true,
    supports_websocket: true,
    supports_cwd: false,
    implicit_url_transport: AgentMcpTransport::Unknown,
    connection_fields: &["oauth", "headersHelper"],
    documentation: "https://code.claude.com/docs/en/mcp",
    config_directory: ".claude",
    shared_skills: false,
    skill_requires_workspace_trust: true,
    json_comments: false,
    declaration_layout: McpDeclarationLayout::ClaudeScopes,
};
pub(crate) static CODEX: NativeCatalogAdapter = NativeCatalogAdapter {
    key: "codex",
    format: ConfigDocumentFormat::Toml,
    table: "mcp_servers",
    file_name: "config.toml",
    workspace_file_name: "config.toml",
    headers: "http_headers",
    http_url: "url",
    explicit_type: false,
    supports_sse: false,
    supports_websocket: false,
    supports_cwd: true,
    implicit_url_transport: AgentMcpTransport::Http,
    connection_fields: &[
        "env_vars",
        "environment_id",
        "experimental_environment",
        "auth",
        "oauth",
        "oauth_resource",
        "bearer_token_env_var",
        "env_http_headers",
        "http_headers_helper",
        "scopes",
    ],
    documentation: "https://developers.openai.com/codex/mcp",
    config_directory: ".codex",
    shared_skills: true,
    skill_requires_workspace_trust: true,
    json_comments: false,
    declaration_layout: McpDeclarationLayout::Root(""),
};
pub(crate) static GEMINI: NativeCatalogAdapter = NativeCatalogAdapter {
    key: "gemini",
    format: ConfigDocumentFormat::Json,
    table: "mcpServers",
    file_name: "settings.json",
    workspace_file_name: "settings.json",
    headers: "headers",
    http_url: "httpUrl",
    explicit_type: false,
    supports_sse: true,
    supports_websocket: false,
    supports_cwd: true,
    implicit_url_transport: AgentMcpTransport::Http,
    connection_fields: &[
        "oauth",
        "authProviderType",
        "targetAudience",
        "targetServiceAccount",
    ],
    documentation: "https://geminicli.com/docs/tools/mcp-server/",
    config_directory: ".gemini",
    shared_skills: true,
    skill_requires_workspace_trust: true,
    json_comments: true,
    declaration_layout: McpDeclarationLayout::Root("mcpServers."),
};
pub(crate) static GROK: NativeCatalogAdapter = NativeCatalogAdapter {
    key: "grok",
    format: ConfigDocumentFormat::Toml,
    table: "mcp_servers",
    file_name: "config.toml",
    workspace_file_name: "config.toml",
    headers: "headers",
    http_url: "url",
    explicit_type: true,
    supports_sse: true,
    supports_websocket: false,
    supports_cwd: true,
    implicit_url_transport: AgentMcpTransport::Http,
    connection_fields: &[
        "bearer_token_env_var",
        "oauth_client_id",
        "oauth_client_secret_env_var",
        "oauth_scopes",
    ],
    documentation: "https://docs.x.ai/build/settings/reference",
    config_directory: ".grok",
    shared_skills: true,
    // Ordinary Skill discovery has no folder-trust gate in Grok. Package
    // children remain excluded by target_sources and keep their parent policy.
    skill_requires_workspace_trust: false,
    json_comments: false,
    declaration_layout: McpDeclarationLayout::Root("mcp_servers."),
};

impl NativeCatalogAdapter {
    pub(super) fn requires_workspace_trust(
        &self,
        category: AgentAssetCategory,
        scope: AgentAssetScope,
    ) -> bool {
        matches!(scope, AgentAssetScope::Workspace | AgentAssetScope::Local)
            && (category != AgentAssetCategory::Skill || self.skill_requires_workspace_trust)
    }

    pub(super) fn parse_document(&self, bytes: &[u8]) -> Option<Value> {
        if self.json_comments {
            parse_jsonc_document(std::str::from_utf8(bytes).ok()?).ok()
        } else {
            config_document::parse(bytes, self.format)
        }
    }

    pub(super) fn target_sources<'a>(
        &self,
        inventory: &'a AgentEnvironmentInventory,
        context: &AgentConfigurationContext,
    ) -> Vec<&'a AgentAssetSource> {
        inventory
            .sources
            .iter()
            .filter(|source| {
                if source.context_id != context.id
                    || source.environment_id != context.environment_id
                    || !matches!(
                        source.scope,
                        AgentAssetScope::User | AgentAssetScope::Workspace
                    )
                    || source.revision.is_symlink
                    || !source.writable
                    || matches!(
                        source.origin,
                        AgentAssetInstallationOrigin::NativePackage
                            | AgentAssetInstallationOrigin::Bundled
                            | AgentAssetInstallationOrigin::Linked
                    )
                {
                    return false;
                }
                let path = Path::new(&source.path);
                let root = Path::new(&context.config_root);
                let workspace = context.workspace_id.as_deref().map(Path::new);
                if source.source_kind == AgentAssetSourceKind::File
                    && source.categories.contains(&AgentAssetCategory::Mcp)
                    && source.origin == AgentAssetInstallationOrigin::ConfigEntry
                {
                    let expected = if source.scope == AgentAssetScope::Workspace {
                        let Some(workspace) = workspace else {
                            return false;
                        };
                        if matches!(self.declaration_layout, McpDeclarationLayout::ClaudeScopes) {
                            workspace.join(self.workspace_file_name)
                        } else {
                            workspace
                                .join(self.config_directory)
                                .join(self.workspace_file_name)
                        }
                    } else if matches!(self.declaration_layout, McpDeclarationLayout::ClaudeScopes)
                        && root == Path::new(&source.allowed_root).join(self.config_directory)
                    {
                        Path::new(&source.allowed_root).join(self.file_name)
                    } else {
                        root.join(self.file_name)
                    };
                    return path == expected;
                }
                if source.source_kind != AgentAssetSourceKind::Directory
                    || source.categories != [AgentAssetCategory::Skill]
                {
                    return false;
                }
                match source.scope {
                    AgentAssetScope::User => {
                        (source.origin == AgentAssetInstallationOrigin::LocalFiles
                            && path == root.join("skills"))
                            || (self.shared_skills
                                && !source.revision.is_missing
                                && source.origin == AgentAssetInstallationOrigin::SharedFiles
                                && path == Path::new(&source.allowed_root).join(".agents/skills"))
                    }
                    AgentAssetScope::Workspace => workspace.is_some_and(|workspace| {
                        (source.origin == AgentAssetInstallationOrigin::LocalFiles
                            && path == workspace.join(self.config_directory).join("skills"))
                            || (self.shared_skills
                                && !source.revision.is_missing
                                && source.origin == AgentAssetInstallationOrigin::SharedFiles
                                && path == workspace.join(".agents/skills"))
                    }),
                    _ => false,
                }
            })
            .collect()
    }

    pub(super) fn target_scopes(
        &self,
        source: &AgentAssetSource,
        context: &AgentConfigurationContext,
    ) -> Vec<AgentAssetScope> {
        let mut scopes = vec![source.scope];
        if matches!(self.declaration_layout, McpDeclarationLayout::ClaudeScopes)
            && source.scope == AgentAssetScope::User
            && source.source_kind == AgentAssetSourceKind::File
            && source.categories.contains(&AgentAssetCategory::Mcp)
            && context.workspace_id.is_some()
        {
            scopes.push(AgentAssetScope::Local);
        }
        scopes
    }

    pub(super) fn mcp_node<'a>(
        &self,
        root: &'a Value,
        context: &AgentConfigurationContext,
        scope: AgentAssetScope,
        name: &str,
    ) -> Option<&'a Value> {
        self.mcp_node_checked(root, context, scope, name)
            .ok()
            .flatten()
    }

    pub(super) fn mcp_node_checked<'a>(
        &self,
        root: &'a Value,
        context: &AgentConfigurationContext,
        scope: AgentAssetScope,
        name: &str,
    ) -> Result<Option<&'a Value>, String> {
        let mut root = root.as_object().ok_or("MCP 配置根不是对象")?;
        if scope == AgentAssetScope::Local
            && matches!(self.declaration_layout, McpDeclarationLayout::ClaudeScopes)
        {
            let Some(projects) = root.get("projects") else {
                return Ok(None);
            };
            let projects = projects.as_object().ok_or("MCP projects 不是对象")?;
            let workspace = context
                .workspace_id
                .as_deref()
                .ok_or("Local MCP 缺少工作区")?;
            let Some(project) = projects.get(workspace) else {
                return Ok(None);
            };
            root = project.as_object().ok_or("Local MCP project 不是对象")?;
        }
        let Some(table) = root.get(self.table) else {
            return Ok(None);
        };
        let table = table.as_object().ok_or("MCP 声明表不是对象")?;
        Ok(table.get(name))
    }

    pub(super) fn patch_mcp_at(
        &self,
        bytes: Option<&[u8]>,
        name: &str,
        definition: &McpDefinition,
        context: &AgentConfigurationContext,
        scope: AgentAssetScope,
    ) -> Result<Vec<u8>, String> {
        if scope != AgentAssetScope::Local {
            return self.patch_mcp(bytes, name, definition);
        }
        if !matches!(self.declaration_layout, McpDeclarationLayout::ClaudeScopes) {
            return Err("此 MCP 生态没有 Local project 节点".to_owned());
        }
        let workspace = context
            .workspace_id
            .as_deref()
            .ok_or("Local MCP 缺少工作区身份")?;
        let mut root = match bytes {
            Some(bytes) => self
                .parse_document(bytes)
                .ok_or("Local MCP 文件无效或含重复字段")?,
            None => Value::Object(Map::new()),
        };
        let projects = root
            .as_object_mut()
            .ok_or("MCP 根不是对象")?
            .entry("projects")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or("MCP projects 不是对象")?;
        let local = projects
            .entry(workspace)
            .or_insert_with(|| Value::Object(Map::new()));
        if !local.is_object() {
            return Err("Local project 配置不是对象".to_owned());
        }
        let before = serde_json::to_vec(local).map_err(|_| "Local MCP 节点无效")?;
        let after = self.patch_mcp(Some(&before), name, definition)?;
        *local = self.parse_document(&after).ok_or("Local MCP 转换无效")?;
        serde_json::to_vec_pretty(&root).map_err(|_| "Local MCP 无法序列化".to_owned())
    }

    pub(crate) fn declaration_value<'a>(
        &self,
        root: &'a Value,
        declaration: &AgentAssetDeclaration,
        context: &AgentConfigurationContext,
    ) -> Result<&'a Value, String> {
        let id = &declaration.native_id;
        let exact_key = |prefix: &str| declaration.declaration_key == format!("{prefix}{id}");
        let value = match &self.declaration_layout {
            McpDeclarationLayout::Root(prefix) if exact_key(prefix) => {
                root.get(self.table).and_then(|table| table.get(id))
            }
            McpDeclarationLayout::ClaudeScopes => match declaration.scope {
                AgentAssetScope::User if exact_key("user:") => {
                    root.get("mcpServers").and_then(|table| table.get(id))
                }
                AgentAssetScope::Local if exact_key("project:") => {
                    // The account file also contains User MCPs with the same
                    // name. A Local declaration can only read its exact native
                    // project node; missing/ambiguous context never falls back.
                    context.workspace_id.as_deref().and_then(|workspace| {
                        root.get("projects")
                            .and_then(|projects| projects.get(workspace))
                            .and_then(|project| project.get("mcpServers"))
                            .and_then(|servers| servers.get(id))
                    })
                }
                AgentAssetScope::Workspace | AgentAssetScope::Managed
                    if exact_key("definition:") =>
                {
                    root.get("mcpServers").and_then(|table| table.get(id))
                }
                AgentAssetScope::Managed if exact_key("managed:") => root
                    .get("managedMcpServers")
                    .and_then(|table| table.get(id)),
                _ => None,
            },
            _ => None,
        };
        value.ok_or_else(|| "无法精确定位该作用域的 MCP 定义，已拒绝收录".to_owned())
    }

    pub(crate) fn patch_mcp(
        &self,
        bytes: Option<&[u8]>,
        name: &str,
        definition: &McpDefinition,
    ) -> Result<Vec<u8>, String> {
        let mut root = match bytes {
            Some(bytes) => self
                .parse_document(bytes)
                .ok_or("目标配置无效或包含重复字段")?,
            None => Value::Object(Map::new()),
        };
        let entry = root
            .as_object_mut()
            .ok_or("目标不是配置对象")?
            .entry(self.table)
            .or_insert_with(|| Value::Object(Map::new()));
        let table = entry.as_object_mut().ok_or("MCP 配置表类型无效")?;
        let replacement = self.merge_connection(table.get(name), definition)?;
        table.insert(name.to_owned(), Value::Object(replacement.clone()));
        if self.format == ConfigDocumentFormat::Json {
            let text =
                std::str::from_utf8(bytes.unwrap_or(b"{}")).map_err(|_| "目标配置不是 UTF-8")?;
            return rewrite_json_document(text, &root, self.json_comments).map(String::into_bytes);
        }
        // Patch one TOML table with toml_edit, preserving all unrelated comments.
        let text =
            std::str::from_utf8(bytes.unwrap_or_default()).map_err(|_| "目标配置不是 UTF-8")?;
        let mut document = text
            .parse::<toml_edit::Document>()
            .map_err(|_| "目标 TOML 无效")?;
        if document.get(self.table).is_none() {
            document[self.table] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let rendered = toml::to_string(&BTreeMap::from([(
            self.table,
            BTreeMap::from([(name, Value::Object(replacement))]),
        )]))
        .map_err(|_| "无法生成 MCP TOML")?;
        let desired = rendered
            .parse::<toml_edit::Document>()
            .map_err(|_| "MCP TOML 转换失败")?;
        let table = document[self.table]
            .as_table_like_mut()
            .ok_or("目标 MCP 表不能安全修改")?;
        table.insert(name, desired[self.table][name].clone());
        Ok(document.to_string().into_bytes())
    }
    pub(super) fn merge_connection(
        &self,
        previous: Option<&Value>,
        definition: &McpDefinition,
    ) -> Result<Map<String, Value>, String> {
        let encoded = self.encode(definition)?;
        let mut replacement = previous
            .map(|value| value.as_object().cloned().ok_or("同名 MCP 不是对象"))
            .transpose()?
            .unwrap_or_default();
        // Keep unknown target-specific settings and its independent enable flag.
        // Replace only the standard definition fields owned by this operation.
        for field in [
            "command",
            "args",
            "url",
            "httpUrl",
            "cwd",
            "env",
            "headers",
            "http_headers",
            "type",
            "transport",
            "urlTemplate",
            "url_template",
        ] {
            replacement.remove(field);
        }
        // Authentication and execution dependencies belong to the connection.
        // Leaving an old token/helper here would silently override the new one.
        for field in self.connection_fields {
            replacement.remove(*field);
        }
        replacement.extend(encoded.as_object().ok_or("MCP 转换失败")?.clone());
        Ok(replacement)
    }
}
