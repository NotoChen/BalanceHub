use super::*;
use serde_json::Value;

pub(super) fn query(
    context: &AgentModelCatalogContext,
    proxy: &EffectiveProxy,
) -> Result<AgentModelCatalog, String> {
    let help = super::super::read_help(&context.cli_path)?;
    for flag in [
        "--input-format",
        "--output-format",
        "--no-session-persistence",
        "--strict-mcp-config",
        "--tools",
    ] {
        if !help.contains(flag) {
            return Err("此 Claude CLI 未提供可用的只读模型查询接口，可输入模型 ID".into());
        }
    }
    let isolation = if help.contains("--safe-mode") {
        "--safe-mode"
    } else if help.contains("--bare") {
        "--bare"
    } else {
        return Err("此 Claude CLI 无法关闭查询时的自定义启动操作，可输入模型 ID".into());
    };
    let mut command = command(context, proxy);
    command
        .args([
            "--print",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--tools",
            "",
            isolation,
        ])
        .env("CLAUDE_CONFIG_DIR", &context.config_root)
        .env("DISABLE_AUTOUPDATER", "1");
    if help.contains("--no-chrome") {
        command.arg("--no-chrome");
    }
    // The official Agent SDK's supportedModels() reads this initialize response.
    // There is no user message, inference request or persisted conversation.
    let input = b"{\"type\":\"control_request\",\"request_id\":\"balancehub-models\",\"request\":{\"subtype\":\"initialize\"}}\n";
    let result = process::run_command_with_input_timeout(
        &mut command,
        input,
        QUERY_TIMEOUT,
        MAX_OUTPUT,
        initialization_received,
    )
    .map_err(|_| "Claude 模型查询无法启动")?;
    if result.stdout_truncated {
        return Err("Claude 模型目录不完整，请重试".into());
    }
    for line in result.stdout.lines() {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if message.get("type").and_then(Value::as_str) != Some("control_response")
            || message
                .pointer("/response/request_id")
                .and_then(Value::as_str)
                != Some("balancehub-models")
            || message.pointer("/response/subtype").and_then(Value::as_str) != Some("success")
        {
            continue;
        }
        if let Some(models) = message
            .pointer("/response/response/models")
            .and_then(Value::as_array)
        {
            let choices = models
                .iter()
                .filter_map(|model| {
                    let value = model.get("value")?.as_str()?;
                    Some(AgentSettingChoice {
                        value: value.into(),
                        label: model
                            .get("displayName")
                            .and_then(Value::as_str)
                            .unwrap_or(value)
                            .into(),
                        description: model
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .into(),
                    })
                })
                .collect::<Vec<_>>();
            if !choices.is_empty() {
                return Ok(catalog(choices, "Claude CLI 原生模型目录"));
            }
        }
    }
    Err(if result.timed_out {
        "Claude 模型查询超时，可稍后刷新"
    } else {
        "Claude 未返回模型目录，请检查原生 CLI 的登录状态；仍可输入模型 ID"
    }
    .into())
}

fn initialization_received(stdout: &str) -> bool {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .any(|message| {
            message.get("type").and_then(Value::as_str) == Some("control_response")
                && message
                    .pointer("/response/request_id")
                    .and_then(Value::as_str)
                    == Some("balancehub-models")
        })
}
