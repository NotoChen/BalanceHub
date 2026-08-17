use super::super::{
    contracts::{
        EnvironmentPatch, LivenessPlan, LivenessRequest, LivenessResponseSource,
        ParsedLivenessOutput, ParsedTokenUsage,
    },
    liveness_support::{extract_u64, token_sum},
};
use serde_json::Value;

pub(super) fn build_plan(request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
    let mut environment = EnvironmentPatch::default();
    environment.remove("CODEX_API_KEY");
    environment.remove("CODEX_ACCESS_TOKEN");
    environment.set("OPENAI_API_KEY", request.api_key.trim());
    environment.set(
        "CODEX_HOME",
        request.isolated_home.to_string_lossy().to_string(),
    );

    let mut args = vec![
        "--ask-for-approval".to_string(),
        "never".to_string(),
        "--sandbox".to_string(),
        "read-only".to_string(),
        "exec".to_string(),
        "--skip-git-repo-check".to_string(),
        "--ephemeral".to_string(),
        "--ignore-user-config".to_string(),
        "--ignore-rules".to_string(),
        "--json".to_string(),
        "-c".to_string(),
        "model_provider=\"balancehub\"".to_string(),
        "-c".to_string(),
        "model_providers.balancehub.identity.name=\"BalanceHub\"".to_string(),
        "-c".to_string(),
        format!(
            "model_providers.balancehub.identity.base_url=\"{}\"",
            escape_toml_string(request.base_url)
        ),
        "-c".to_string(),
        "model_providers.balancehub.wire_api=\"responses\"".to_string(),
        "-c".to_string(),
        "model_providers.balancehub.env_key=\"OPENAI_API_KEY\"".to_string(),
        "-c".to_string(),
        "model_providers.balancehub.requires_openai_auth=true".to_string(),
    ];
    if !request.model.trim().is_empty() {
        args.extend(["-m".to_string(), request.model.trim().to_string()]);
    }
    args.extend([
        "-o".to_string(),
        request.output_path.to_string_lossy().to_string(),
        request.prompt.to_string(),
    ]);

    Ok(LivenessPlan {
        args,
        environment,
        files: Vec::new(),
        response_source: LivenessResponseSource::File(request.output_path.to_path_buf()),
    })
}

pub(super) fn parse_output(response_output: &str, stdout: &str) -> ParsedLivenessOutput {
    let mut usage = ParsedTokenUsage::default();
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(metrics) = value.get("usage") else {
            continue;
        };
        usage.input_tokens = extract_u64(metrics, "input_tokens");
        usage.cached_input_tokens = extract_u64(metrics, "cached_input_tokens");
        usage.output_tokens = extract_u64(metrics, "output_tokens");
        usage.reasoning_output_tokens = extract_u64(metrics, "reasoning_output_tokens");
        usage.total_tokens = token_sum(&[usage.input_tokens, usage.output_tokens]);
    }
    ParsedLivenessOutput {
        response: response_output.trim().chars().take(240).collect(),
        error: None,
        usage,
    }
}

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
