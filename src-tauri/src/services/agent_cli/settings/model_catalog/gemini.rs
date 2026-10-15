use super::*;
use serde_json::Value;

pub(super) fn query(
    context: &AgentModelCatalogContext,
    proxy: &EffectiveProxy,
) -> Result<AgentModelCatalog, String> {
    // Import only model metadata from the selected installation. ACP session/new
    // would initialize a conversation, hooks and MCP just to obtain its model list.
    let mut command = Command::new(if cfg!(windows) { "node.exe" } else { "node" });
    command.args([
        "--input-type=module",
        "--eval",
        include_str!("gemini_catalog.mjs"),
        &context.cli_path,
    ]);
    if let Some(path) = agent_cli::runtime_path_for(Path::new(&context.cli_path)) {
        command.env("PATH", path);
    }
    network::apply_proxy_env(&mut command, proxy);
    command.env("NODE_NO_WARNINGS", "1");
    let text = output(&mut command)
        .map_err(|_| "未能读取所选 Gemini 安装包的模型目录；仍可输入模型 ID")?;
    let models = text
        .lines()
        .find_map(|line| line.strip_prefix("BALANCEHUB_MODELS="))
        .and_then(|line| serde_json::from_str::<Value>(line).ok())
        .and_then(|value| value.as_array().cloned())
        .ok_or("此 Gemini 安装未提供可读取的模型目录；仍可输入模型 ID")?;
    let choices = models
        .iter()
        .filter_map(|model| {
            let id = model.get("value")?.as_str()?;
            Some(AgentSettingChoice {
                value: id.into(),
                label: model
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or(id)
                    .into(),
                description: String::new(),
            })
        })
        .collect::<Vec<_>>();
    if choices.is_empty() {
        return Err("Gemini 安装包暂未返回模型目录；仍可输入模型 ID".into());
    }
    Ok(catalog(choices, "所选 Gemini CLI 的模型目录"))
}
