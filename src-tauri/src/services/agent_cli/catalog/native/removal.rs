use super::{McpDeclarationLayout, NativeCatalogAdapter};
use crate::{
    models::*,
    services::agent_cli::{
        config_support::rewrite_json_document, environment::config_document::ConfigDocumentFormat,
    },
};

impl NativeCatalogAdapter {
    pub(in crate::services::agent_cli::catalog) fn remove_mcp_at(
        &self,
        bytes: &[u8],
        name: &str,
        context: &AgentConfigurationContext,
        scope: AgentAssetScope,
    ) -> Result<Vec<u8>, String> {
        let mut root = self
            .parse_document(bytes)
            .ok_or("MCP 配置无效或含重复字段")?;
        self.mcp_node_checked(&root, context, scope, name)?
            .ok_or("所选 MCP 已不存在")?;
        let mut node = &mut root;
        if scope == AgentAssetScope::Local
            && matches!(self.declaration_layout, McpDeclarationLayout::ClaudeScopes)
        {
            let workspace = context
                .workspace_id
                .as_deref()
                .ok_or("项目本地 MCP 缺少工作区")?;
            node = node
                .get_mut("projects")
                .and_then(|projects| projects.get_mut(workspace))
                .ok_or("项目 MCP 节点已变化")?;
        }
        node.get_mut(self.table)
            .and_then(serde_json::Value::as_object_mut)
            .ok_or("MCP 声明表不是对象")?
            .remove(name)
            .ok_or("所选 MCP 已不存在")?;
        let text = std::str::from_utf8(bytes).map_err(|_| "MCP 文件不是 UTF-8")?;
        let after = if self.format == ConfigDocumentFormat::Toml {
            let mut document = text
                .parse::<toml_edit::Document>()
                .map_err(|_| "TOML 文件无效")?;
            document
                .get_mut(self.table)
                .and_then(toml_edit::Item::as_table_like_mut)
                .ok_or("MCP TOML 表无效")?
                .remove(name)
                .ok_or("所选 MCP 已不存在")?;
            document.to_string()
        } else {
            rewrite_json_document(text, &root, self.json_comments)?
        };
        if self.parse_document(after.as_bytes()) != Some(root) {
            return Err("移除后的 MCP 文档验证失败".to_owned());
        }
        Ok(after.into_bytes())
    }
}
