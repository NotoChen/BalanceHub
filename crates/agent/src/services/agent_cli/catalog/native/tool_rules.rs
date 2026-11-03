//! Tool restrictions are native policy, separate from the portable connection.
use super::NativeCatalogAdapter;
use crate::{
    models::*,
    services::agent_cli::config_support::{rewrite_json_document, text_document},
};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub(crate) enum ToolRuleFormat {
    ClaudeDeny,
    Entry {
        excluded: &'static str,
        included: &'static str,
        include_arguments: bool,
    },
}

impl NativeCatalogAdapter {
    pub(crate) fn tool_rule_format(&self) -> Result<ToolRuleFormat, String> {
        match self.key {
            "claude" => Ok(ToolRuleFormat::ClaudeDeny),
            "codex" => Ok(ToolRuleFormat::Entry {
                excluded: "disabled_tools",
                included: "enabled_tools",
                include_arguments: false,
            }),
            "gemini" => Ok(ToolRuleFormat::Entry {
                excluded: "excludeTools",
                included: "includeTools",
                include_arguments: true,
            }),
            _ => Err("此 Agent 尚无已核对的单工具限制配置，可使用服务端的工具设置".to_owned()),
        }
    }

    pub(crate) fn rewrite_tool_rules(
        &self,
        original: Option<&[u8]>,
        root: &Value,
        server: &str,
        rules: &[String],
    ) -> Result<Vec<u8>, String> {
        let original =
            std::str::from_utf8(original.unwrap_or(b"{}")).map_err(|_| "工具规则文件不是 UTF-8")?;
        let format = self.tool_rule_format()?;
        if let ToolRuleFormat::Entry { excluded, .. } = format {
            if self.format == super::ConfigDocumentFormat::Toml {
                let mut document = text_document::body(original)
                    .parse::<toml_edit::Document>()
                    .map_err(|_| "工具规则 TOML 无效")?;
                let entry = document
                    .get_mut(self.table)
                    .and_then(toml_edit::Item::as_table_like_mut)
                    .and_then(|table| table.get_mut(server))
                    .and_then(toml_edit::Item::as_table_like_mut)
                    .ok_or("MCP 配置节点已变化")?;
                if rules.is_empty() {
                    entry.remove(excluded);
                } else {
                    let array = rules
                        .iter()
                        .map(String::as_str)
                        .collect::<toml_edit::Array>();
                    entry.insert(excluded, toml_edit::value(array));
                }
                let result =
                    text_document::restore_layout(original, &document.to_string()).into_bytes();
                if self.parse_document(&result).as_ref() != Some(root) {
                    return Err("工具规则 TOML 写入后校验失败".to_owned());
                }
                return Ok(result);
            }
        }
        let text = if text_document::body(original).trim().is_empty() {
            "{}"
        } else {
            original
        };
        rewrite_json_document(text, root, self.json_comments).map(String::into_bytes)
    }
}

impl ToolRuleFormat {
    pub(crate) fn settings_path(
        self,
        context: &AgentConfigurationContext,
        scope: AgentAssetScope,
    ) -> Result<Option<PathBuf>, String> {
        if !matches!(self, Self::ClaudeDeny) {
            return Ok(None);
        }
        let path = match scope {
            AgentAssetScope::User => Path::new(&context.config_root).join("settings.json"),
            AgentAssetScope::Workspace | AgentAssetScope::Local => Path::new(
                context
                    .workspace_id
                    .as_deref()
                    .ok_or("项目 MCP 缺少项目目录")?,
            )
            .join(".claude")
            .join(if scope == AgentAssetScope::Local {
                "settings.local.json"
            } else {
                "settings.json"
            }),
            _ => return Err("组织管理的 MCP 规则只能由其管理方修改".to_owned()),
        };
        Ok(Some(path))
    }
    pub(crate) fn rule(self, server: &str, tool: &str) -> String {
        fn normalized(value: &str) -> String {
            value
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect()
        }
        match self {
            Self::ClaudeDeny => format!("mcp__{}__{}", normalized(server), normalized(tool)),
            Self::Entry { .. } => tool.to_owned(),
        }
    }
    pub(crate) fn current(
        self,
        root: &Value,
        table: &str,
        server: &str,
    ) -> Result<Vec<String>, String> {
        if !root.is_object() {
            return Err("工具配置根节点不是对象".to_owned());
        }
        if matches!(self, Self::ClaudeDeny)
            && root
                .get("permissions")
                .is_some_and(|value| !value.is_object())
        {
            return Err("原配置的 permissions 不是对象，请先修复原文".to_owned());
        }
        if matches!(self, Self::Entry { .. })
            && !root
                .get(table)
                .and_then(|node| node.get(server))
                .is_some_and(Value::is_object)
        {
            return Err("MCP 工具规则父节点不是对象".to_owned());
        }
        let node = match self {
            Self::ClaudeDeny => root.get("permissions").and_then(|node| node.get("deny")),
            Self::Entry { excluded, .. } => root
                .get(table)
                .and_then(|node| node.get(server))
                .and_then(|node| node.get(excluded)),
        };
        strings(node).map(Option::unwrap_or_default)
    }
    pub(crate) fn excluded_by_include(
        self,
        root: &Value,
        table: &str,
        server: &str,
        tool: &str,
    ) -> Result<bool, String> {
        let Self::Entry {
            included,
            include_arguments,
            ..
        } = self
        else {
            return Ok(false);
        };
        let list = strings(
            root.get(table)
                .and_then(|node| node.get(server))
                .and_then(|node| node.get(included)),
        )?;
        Ok(list.is_some_and(|list| {
            !list.iter().any(|item| {
                item == tool || (include_arguments && item.starts_with(&format!("{tool}(")))
            })
        }))
    }
    pub(crate) fn matches(self, rule: &str, server: &str, tool: &str) -> bool {
        let expected = self.rule(server, tool);
        if matches!(self, Self::Entry { .. }) {
            return rule == expected;
        }
        let server_rule = self.rule(server, "");
        if Some(rule) == server_rule.strip_suffix("__") {
            return true;
        }
        // Claude documents '*' for tool names; argument-specific permission
        // rules are intentionally not represented as complete tool disablement.
        wildcard_matches(rule, &expected)
    }
    pub(crate) fn replace(
        self,
        root: &mut Value,
        table: &str,
        server: &str,
        rules: &[String],
    ) -> Result<(), String> {
        let (node, key) = match self {
            Self::ClaudeDeny => {
                let root = root.as_object_mut().ok_or("设置根节点不是对象")?;
                (
                    root.entry("permissions")
                        .or_insert_with(|| Value::Object(Map::new())),
                    "deny",
                )
            }
            Self::Entry { excluded, .. } => (
                root.get_mut(table)
                    .and_then(|node| node.get_mut(server))
                    .ok_or("MCP 节点不存在")?,
                excluded,
            ),
        };
        let node = node.as_object_mut().ok_or("工具规则父节点不是对象")?;
        if rules.is_empty() {
            node.remove(key);
        } else {
            node.insert(key.to_owned(), serde_json::json!(rules));
        }
        Ok(())
    }
}
// Only '*' is a documented wildcard. Avoid compiling a regex for every pair
// of native rules and tools when a server publishes thousands of tools.
fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let mut parts = pattern.split('*').peekable();
    let first = parts.next().unwrap_or_default();
    let Some(mut remaining) = value.strip_prefix(first) else {
        return false;
    };
    if parts.peek().is_none() {
        return remaining.is_empty();
    }
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            return remaining.ends_with(part);
        }
        let Some(index) = remaining.find(part) else {
            return false;
        };
        remaining = &remaining[index + part.len()..];
    }
    true
}
fn strings(value: Option<&Value>) -> Result<Option<Vec<String>>, String> {
    value
        .map(|value| {
            value
                .as_array()
                .ok_or("工具限制不是字符串列表")?
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_owned)
                        .ok_or("工具限制含有非字符串项")
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()
        .map_err(str::to_owned)
}
