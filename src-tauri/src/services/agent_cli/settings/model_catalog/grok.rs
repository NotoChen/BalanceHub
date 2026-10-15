use super::*;

pub(super) fn query(
    context: &AgentModelCatalogContext,
    proxy: &EffectiveProxy,
) -> Result<AgentModelCatalog, String> {
    let mut command = command(context, proxy);
    command.arg("models").env("GROK_HOME", &context.config_root);
    let text = output(&mut command)?;
    if text.to_ascii_lowercase().contains("not authenticated") {
        return Err("Grok 尚未登录，登录后可刷新模型目录；仍可输入模型 ID".into());
    }
    let mut in_models = false;
    let mut choices = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.to_ascii_lowercase().starts_with("available models") {
            in_models = true;
            continue;
        }
        if !in_models {
            continue;
        }
        let line = line.trim_start_matches(['-', '*', '•', '|', '│', ' ']);
        let Some(id) = line
            .split(|c: char| c.is_whitespace() || c == '|' || c == '│')
            .next()
        else {
            continue;
        };
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_/.:".contains(c))
            || !id.chars().any(|c| c.is_ascii_alphabetic())
            || !id.contains(['-', '_', '/'])
            || id.ends_with(':')
        {
            continue;
        }
        choices.push(AgentSettingChoice {
            value: id.into(),
            label: id.into(),
            description: String::new(),
        });
    }
    if choices.is_empty() {
        return Err("Grok 暂未返回可识别的模型目录；仍可输入模型 ID".into());
    }
    Ok(catalog(choices, "Grok CLI 模型目录"))
}
