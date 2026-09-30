use crate::models::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct StoredDefinition {
    pub version: u64,
    pub payload: DefinitionPayload,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) enum DefinitionPayload {
    Mcp(McpDefinition),
    Skill(BTreeMap<String, PackageFile>),
    Hook(super::hook_definition::HookDefinitions),
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PackageFile {
    pub bytes: Vec<u8>,
    pub executable: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct McpDefinition {
    pub transport: AgentMcpTransport,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub cwd: Option<String>,
    pub environment: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    /// Native connection dependencies (authentication or execution environment).
    /// Local tool, timeout and enablement policies are not shared definitions.
    pub native_options: BTreeMap<String, serde_json::Value>,
    pub native_adapter: Option<String>,
}

impl McpDefinition {
    pub(super) fn as_input(&self) -> AgentCatalogMcpInput {
        AgentCatalogMcpInput {
            transport: Some(self.transport),
            command: self.command.clone(),
            args: self.args.clone(),
            url: self.url.clone(),
            cwd: self.cwd.clone(),
            environment: self.environment.clone(),
            headers: self.headers.clone(),
            connection_options: self.shared_connection_options(),
        }
    }
}

impl DefinitionPayload {
    pub(super) fn comparison_bytes(&self) -> usize {
        match self {
            Self::Skill(files) => files.values().map(|file| file.bytes.len()).sum(),
            _ => serde_json::to_vec(self).map_or(0, |bytes| bytes.len()),
        }
    }

    pub(super) fn binding_fingerprint(&self, kind: AgentCliKind) -> Option<String> {
        match self {
            Self::Hook(values) => values.get(&kind).map(super::hook_definition::fingerprint),
            _ => Some(self.fingerprint()),
        }
    }
    pub(super) fn fingerprint(&self) -> String {
        // Private evidence only; never return a credential-derived digest.
        let bytes = match self {
            Self::Mcp(value) => serde_json::to_vec(&value.connection_document()),
            _ => serde_json::to_vec(self),
        };
        super::digest(&bytes.unwrap_or_default())
    }

    pub(super) fn summary(&self) -> Vec<String> {
        match self {
            Self::Skill(files) => vec![format!(
                "完整 Skill 包：{} 个文件，{} 字节",
                files.len(),
                files.values().map(|file| file.bytes.len()).sum::<usize>()
            )],
            Self::Mcp(definition) => vec![
                format!(
                    "{:?}；{} 个环境变量；{} 个请求头",
                    definition.transport,
                    definition.environment.len(),
                    definition.headers.len()
                ),
                if definition.connection_options().is_empty() {
                    "按 MCP 连接字段比较；Agent 本地策略单独保留".to_owned()
                } else {
                    "含认证或运行环境依赖，目标按具体字段检查".to_owned()
                },
            ],
            Self::Hook(values) => values
                .keys()
                .map(|kind| {
                    format!(
                        "{} 原生 Hook 定义",
                        crate::services::agent_cli::definition(*kind).label
                    )
                })
                .collect(),
        }
    }
}

pub(super) fn input_payload(
    request: &AgentCatalogSaveRequest,
    previous: Option<&StoredDefinition>,
) -> Result<DefinitionPayload, String> {
    if request.category == AgentAssetCategory::Hook {
        validate_hook_display_name(&request.name)?;
    } else {
        validate_name(&request.name)?;
    }
    if (request.category != AgentAssetCategory::Mcp && request.mcp.is_some())
        || (request.category != AgentAssetCategory::Skill && request.skill_markdown.is_some())
        || (request.category != AgentAssetCategory::Hook && request.hook.is_some())
    {
        return Err("共享定义包含其他资产类别的字段".to_owned());
    }
    match request.category {
        AgentAssetCategory::Hook => {
            let input = request.hook.as_ref().ok_or("请填写 Hook 原生定义")?;
            super::hook_definition::input(input).map(DefinitionPayload::Hook)
        }
        AgentAssetCategory::Mcp => {
            let input = request.mcp.as_ref().ok_or("请填写 MCP 定义")?;
            let old = previous.and_then(|old| match &old.payload {
                DefinitionPayload::Mcp(value) => Some(value),
                _ => None,
            });
            let mut definition = McpDefinition {
                transport: input.transport.unwrap_or_else(|| {
                    inferred_mcp_transport(input.command.as_deref(), AgentMcpTransport::Http)
                }),
                command: input.command.clone(),
                args: input.args.clone(),
                url: input.url.clone(),
                cwd: input.cwd.clone(),
                environment: named_values(&input.environment)?,
                headers: named_values(&input.headers)?,
                native_options: BTreeMap::new(),
                native_adapter: old.and_then(|old| old.native_adapter.clone()),
            };
            definition.set_shared_connection_options(&input.connection_options)?;
            validate_mcp(&definition)?;
            Ok(DefinitionPayload::Mcp(definition))
        }
        AgentAssetCategory::Skill => {
            let mut files = match previous.map(|old| &old.payload) {
                Some(DefinitionPayload::Skill(files)) => files.clone(),
                _ => BTreeMap::new(),
            };
            if let Some(markdown) = &request.skill_markdown {
                if markdown.is_empty() || markdown.len() > super::package::MAX_FILE_BYTES {
                    return Err("Skill 内容为空或超过 512 KiB".to_owned());
                }
                validate_skill(markdown.as_bytes())?;
                files.insert(
                    "SKILL.md".to_owned(),
                    PackageFile {
                        bytes: markdown.as_bytes().to_vec(),
                        executable: false,
                    },
                );
            }
            if !files.contains_key("SKILL.md") {
                return Err("新建 Skill 必须填写含 name、description 的 SKILL.md".to_owned());
            }
            Ok(DefinitionPayload::Skill(files))
        }
        _ => Err("该原生生态不支持通用共享定义，请使用原生资产操作".to_owned()),
    }
}

fn validate_hook_display_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.trim() != name
        || name.len() > 128
        || name.chars().any(char::is_control)
    {
        return Err("Hook 名称不能为空或包含首尾空白、控制字符，最多 128 字节".to_owned());
    }
    Ok(())
}

pub(crate) fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 128
        || name == "."
        || name == ".."
        || !name
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '-' | '_' | '.'))
    {
        return Err("资产名称只能包含文字、数字、点、短横线和下划线，最多 128 字节".to_owned());
    }
    Ok(())
}

pub(crate) fn validate_skill(bytes: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "SKILL.md 必须是 UTF-8")?;
    let normalized = text.replace("\r\n", "\n");
    let body = normalized
        .strip_prefix("---\n")
        .ok_or("Skill 缺少 YAML frontmatter")?;
    let mut end = 0;
    let mut terminated = false;
    for line in body.split_inclusive('\n').take(512) {
        if line.trim_end() == "---" {
            terminated = true;
            break;
        }
        end += line.len();
        if end > 16 * 1024 {
            break;
        }
    }
    if !terminated || end > 16 * 1024 {
        return Err("Skill frontmatter 缺少结束标记或超过 16 KiB / 512 行".to_owned());
    }
    let metadata: serde_yaml_ng::Mapping = serde_yaml_ng::from_str(&body[..end])
        .map_err(|_| "Skill frontmatter 不是有效的 YAML 对象")?;
    for field in ["name", "description"] {
        if metadata
            .get(serde_yaml_ng::Value::String(field.to_owned()))
            .and_then(serde_yaml_ng::Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(format!("Skill 缺少有效的 {field}"));
        }
    }
    Ok(())
}

pub(crate) fn inferred_mcp_transport(
    command: Option<&str>,
    remote_default: AgentMcpTransport,
) -> AgentMcpTransport {
    if command.is_some() {
        AgentMcpTransport::Stdio
    } else {
        remote_default
    }
}

pub(crate) fn validate_mcp(value: &McpDefinition) -> Result<(), String> {
    match value.transport {
        AgentMcpTransport::Stdio
            if value
                .command
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty())
                && value.url.is_none() => {}
        AgentMcpTransport::Http | AgentMcpTransport::Sse | AgentMcpTransport::WebSocket
            if value.command.is_none()
                && value
                    .url
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty()) => {}
        _ => return Err("MCP 传输与命令/地址不匹配".to_owned()),
    }
    if let Some(url) = &value.url {
        let expanded = super::native::validation_url(
            value.native_adapter.as_deref().unwrap_or(""),
            url,
            value.transport == AgentMcpTransport::WebSocket,
        );
        if !reqwest::Url::parse(&expanded).is_ok_and(|url| {
            url.host_str().is_some()
                && if value.transport == AgentMcpTransport::WebSocket {
                    matches!(url.scheme(), "ws" | "wss")
                } else {
                    matches!(url.scheme(), "http" | "https")
                }
        }) {
            return Err(
                "MCP 服务地址与传输不匹配：HTTP/SSE 使用 http(s)，WebSocket 使用 ws(s)".to_owned(),
            );
        }
    }
    if value.transport == AgentMcpTransport::Stdio && !value.headers.is_empty() {
        return Err("stdio 通过环境变量传递参数，不接受 HTTP 请求头".to_owned());
    }
    if value.transport != AgentMcpTransport::Stdio
        && (!value.args.is_empty() || !value.environment.is_empty() || value.cwd.is_some())
    {
        return Err("远程 MCP 不启动本地进程，不能包含 args、env 或 cwd".to_owned());
    }
    let mut headers = BTreeMap::new();
    for (name, content) in &value.headers {
        if headers
            .insert(name.to_ascii_lowercase(), content)
            .is_some_and(|previous| previous != content)
        {
            return Err("MCP 请求头包含大小写不同但值冲突的同名字段".to_owned());
        }
    }
    if value.args.len() > 256
        || value.environment.len() > 128
        || value.headers.len() > 128
        || serde_json::to_vec(value).map_err(|_| "定义无效")?.len() > super::package::MAX_FILE_BYTES
    {
        return Err("MCP 定义超过有界限制".to_owned());
    }
    if value
        .command
        .iter()
        .chain(value.args.iter())
        .chain(value.url.iter())
        .chain(value.cwd.iter())
        .any(|value| value.contains('\0'))
    {
        return Err("MCP 定义包含无效字符".to_owned());
    }
    Ok(())
}

fn named_values(input: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>, String> {
    if input
        .keys()
        .any(|key| key.is_empty() || key.contains(['\0', '\n', '\r']))
    {
        return Err("环境变量/请求头名称无效".to_owned());
    }
    Ok(input.clone())
}

pub(super) fn public_definition(
    id: &str,
    name: &str,
    definition: &StoredDefinition,
) -> AgentCatalogDefinition {
    let (category, mcp, hook, files) = match &definition.payload {
        DefinitionPayload::Mcp(value) => {
            let public = value.as_input();
            (AgentAssetCategory::Mcp, Some(public), None, Vec::new())
        }
        DefinitionPayload::Skill(files) => (
            AgentAssetCategory::Skill,
            None,
            None,
            files
                .iter()
                .map(|(path, file)| AgentCatalogPackageFile {
                    path: path.clone(),
                    size_bytes: file.bytes.len(),
                })
                .collect(),
        ),
        DefinitionPayload::Hook(values) => (
            AgentAssetCategory::Hook,
            None,
            Some(super::hook_definition::public(values)),
            Vec::new(),
        ),
    };
    AgentCatalogDefinition {
        asset_id: id.to_owned(),
        name: name.to_owned(),
        category,
        version: definition.version,
        mcp,
        hook,
        skill_markdown: match &definition.payload {
            DefinitionPayload::Skill(files) => files
                .get("SKILL.md")
                .and_then(|file| std::str::from_utf8(&file.bytes).ok())
                .map(str::to_owned),
            _ => None,
        },
        files,
        notes: vec!["保存定义只更新本地共享库；选择应用目标后才会写入对应 Agent。".to_owned()],
    }
}
