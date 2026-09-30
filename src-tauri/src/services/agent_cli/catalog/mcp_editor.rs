//! Pure form/native draft conversion. Persistence uses the existing edit plan.
use super::definition::{
    inferred_mcp_transport, input_payload, DefinitionPayload, StoredDefinition,
};
use crate::{
    models::*,
    services::agent_cli::{config_support::rewrite_json_document, configuration::native_support},
};

fn parse(text: &str, format: AgentConfigurationFormat) -> Result<serde_json::Value, String> {
    if text.len() > super::package::MAX_FILE_BYTES {
        return Err("MCP 配置超过 512 KiB".to_owned());
    }
    if !matches!(
        format,
        AgentConfigurationFormat::Json
            | AgentConfigurationFormat::Jsonc
            | AgentConfigurationFormat::Toml
    ) {
        return Err("MCP 连接需要 JSON 或 TOML 格式".to_owned());
    }
    native_support::parse(text, format).map_err(|error| error.message)
}

pub(crate) fn read(
    text: &str,
    format: AgentConfigurationFormat,
    kind: Option<AgentCliKind>,
) -> Result<AgentMcpFormRead, String> {
    let value = parse(text, format)?;
    if let Some(kind) = kind {
        let input = crate::services::agent_cli::definition(kind)
            .environment
            .catalog_adapter()
            .ok_or("此 Agent 尚无 MCP 表单映射")?
            .decode(&value)
            .map(|definition| definition.as_input())?;
        return Ok(form_read(input, Some(kind)));
    }
    let mut input: AgentCatalogMcpInput =
        serde_json::from_value(value).map_err(|error| format!("无法转为连接表单：{error}"))?;
    input.transport = Some(input.transport.unwrap_or_else(|| {
        inferred_mcp_transport(input.command.as_deref(), AgentMcpTransport::Http)
    }));
    if input.transport == Some(AgentMcpTransport::Unknown) {
        return Err("此传输方式尚无表单映射，请使用原文编辑".to_owned());
    }
    Ok(form_read(input, kind))
}

fn form_read(input: AgentCatalogMcpInput, target: Option<AgentCliKind>) -> AgentMcpFormRead {
    use super::native::NativeCatalogAdapter;
    let agents = crate::services::agent_cli::definitions();
    let rule =
        |key: &str, local: bool, remote: bool, supports: &dyn Fn(&NativeCatalogAdapter) -> bool| {
            let eligible = agents
                .iter()
                .filter(|agent| agent.environment.catalog_adapter().is_some_and(supports))
                .collect::<Vec<_>>();
            AgentMcpFormRule {
                key: key.to_owned(),
                local,
                remote,
                supported: target
                    .is_none_or(|kind| eligible.iter().any(|agent| agent.kind == kind)),
                support_label: if eligible.len()
                    == agents
                        .iter()
                        .filter(|agent| agent.environment.catalog_adapter().is_some())
                        .count()
                {
                    String::new()
                } else {
                    format!(
                        "可用于：{}",
                        eligible
                            .iter()
                            .map(|agent| agent.label)
                            .collect::<Vec<_>>()
                            .join("、")
                    )
                },
            }
        };
    let keys = ["command", "args", "env", "cwd", "url", "headers"]
        .into_iter()
        .chain(NativeCatalogAdapter::form_option_keys());
    let fields = keys
        .map(|key| {
            let local = matches!(
                key,
                "command" | "args" | "env" | "cwd" | "environmentVariables"
            );
            rule(key, local, !local, &|adapter| {
                adapter.supports_form_field(key)
            })
        })
        .collect();
    let transports = ["stdio", "http", "sse", "webSocket"]
        .into_iter()
        .map(|key| {
            rule(key, key == "stdio", key != "stdio", &|adapter| match key {
                "sse" => adapter.supports_sse,
                "webSocket" => adapter.supports_websocket,
                _ => true,
            })
        })
        .collect();
    AgentMcpFormRead {
        input,
        fields,
        transports,
        target_label: target.map(|kind| {
            crate::services::agent_cli::definition(kind)
                .label
                .to_owned()
        }),
    }
}

pub(crate) fn render(
    input: AgentCatalogMcpInput,
    text: &str,
    format: AgentConfigurationFormat,
    kind: Option<AgentCliKind>,
) -> Result<String, String> {
    let request = AgentCatalogSaveRequest {
        asset_id: None,
        expected_version: None,
        category: AgentAssetCategory::Mcp,
        name: "mcp".to_owned(),
        mcp: Some(input),
        skill_markdown: None,
        hook: None,
    };
    let Some(kind) = kind else {
        let DefinitionPayload::Mcp(definition) = input_payload(&request, None)? else {
            return Err("MCP 表单类型不匹配".to_owned());
        };
        return serde_json::to_string_pretty(&definition.as_input())
            .map_err(|_| "无法生成 MCP 配置草稿".to_owned());
    };
    let root = parse(text, format)?;
    let adapter = crate::services::agent_cli::definition(kind)
        .environment
        .catalog_adapter()
        .ok_or("此 Agent 尚无 MCP 表单映射")?;
    let previous = StoredDefinition {
        version: 0,
        payload: DefinitionPayload::Mcp(adapter.decode(&root)?),
    };
    let DefinitionPayload::Mcp(definition) = input_payload(&request, Some(&previous))? else {
        return Err("MCP 表单类型不匹配".to_owned());
    };
    let replacement =
        serde_json::Value::Object(adapter.merge_connection(Some(&root), &definition)?);
    if replacement == root {
        return Ok(text.to_owned());
    }
    let rendered = if format == AgentConfigurationFormat::Toml {
        let mut document = text
            .parse::<toml_edit::Document>()
            .map_err(|_| "原 TOML 无法读取")?;
        let desired = toml::to_string(&replacement)
            .map_err(|_| "无法生成 MCP TOML")?
            .parse::<toml_edit::Document>()
            .map_err(|_| "生成的 TOML 无效")?;
        let keys = document
            .iter()
            .map(|(key, _)| key.to_owned())
            .collect::<Vec<_>>();
        for key in keys {
            if replacement.get(&key).is_none() {
                document.remove(&key);
            }
        }
        for (key, item) in desired.iter() {
            if root.get(key) != replacement.get(key) {
                document.insert(key, item.clone());
            }
        }
        document.to_string()
    } else {
        rewrite_json_document(
            text,
            &replacement,
            format == AgentConfigurationFormat::Jsonc,
        )?
    };
    if parse(&rendered, format)? != replacement {
        return Err("表单转换改变了其他配置，草稿未更新".to_owned());
    }
    Ok(rendered)
}
