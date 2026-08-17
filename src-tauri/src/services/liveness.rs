mod process;
mod prompt;

pub use prompt::{effective_interval, preview_prompts};

use crate::{
    limits,
    models::{provider_domain, AgentCliKind, AppSettings, LivenessRecord, Provider},
    network,
    services::agent_cli::{
        self,
        contracts::{EnvironmentPatch, LivenessRequest, LivenessResponseSource},
    },
    util::unix_millis as now_millis,
};
use process::wait_with_output_timeout;
use prompt::{effective_cli_kind, effective_model, effective_timeout, select_prompt};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub struct LivenessRunner;

#[derive(Debug, Clone)]
pub struct LivenessContext {
    pub cli_kind: AgentCliKind,
    pub cli_path: String,
    pub model: String,
    pub base_url: String,
    pub prompt: String,
    pub timeout_seconds: u64,
    pub command_preview: String,
}

impl LivenessRunner {
    pub fn build_context(
        settings: &AppSettings,
        provider: &Provider,
        prompt_override: Option<String>,
    ) -> Result<LivenessContext, String> {
        if !provider_domain::auth::has_api_key(provider) {
            return Err("缺少 API Key，测活需要 API Key".to_string());
        }
        let model = effective_model(settings, provider);
        let prompt = prompt_override
            .and_then(|value| {
                let value = value.trim().to_string();
                (!value.is_empty()).then_some(value)
            })
            .unwrap_or_else(|| select_prompt(settings, provider));
        let timeout_seconds = effective_timeout(settings, provider);
        let cli_kind = effective_cli_kind(settings, provider);
        let definition = agent_cli::definition(cli_kind);
        let adapter = definition
            .liveness()
            .ok_or_else(|| format!("{} 当前不支持测活", definition.label))?;
        let cli = agent_cli::find(settings, cli_kind, true)?;
        let base_url = agent_cli::provider_base_url(cli_kind, provider);
        let preview_home = PathBuf::from(format!(
            "/tmp/balancehub-agent-liveness-{}-<random>",
            cli_kind.key()
        ));
        let preview_output = preview_home.join("response.txt");
        let preview_plan = adapter.build_plan(LivenessRequest {
            api_key: "***",
            base_url: &base_url,
            model: &model,
            prompt: &prompt,
            timeout_seconds,
            isolated_home: &preview_home,
            output_path: &preview_output,
        })?;
        let command_preview =
            format_command_preview(&cli.path, &preview_plan.environment, &preview_plan.args);

        Ok(LivenessContext {
            cli_kind,
            cli_path: cli.path,
            model,
            base_url,
            prompt,
            timeout_seconds,
            command_preview,
        })
    }

    pub fn run(
        settings: &AppSettings,
        provider: &Provider,
        prompt_override: Option<String>,
        automatic: bool,
    ) -> LivenessRecord {
        let checked_at = now_millis().to_string();
        let source = if automatic { "automatic" } else { "manual" }.to_string();
        let context = match Self::build_context(settings, provider, prompt_override) {
            Ok(context) => context,
            Err(message) => {
                return failure_record(checked_at, source, message, String::new(), String::new());
            }
        };
        let definition = agent_cli::definition(context.cli_kind);
        let Some(adapter) = definition.liveness() else {
            return failure_record(
                checked_at,
                source,
                format!("{} 当前不支持测活", definition.label),
                context.command_preview,
                context.prompt,
            );
        };
        let isolated_home = unique_liveness_home(provider, context.cli_kind);
        let output_path = isolated_home.join("response.txt");
        if let Err(err) = fs::create_dir_all(&isolated_home) {
            return failure_record(
                checked_at,
                source,
                format!("创建测活 CLI 临时目录失败: {err}"),
                context.command_preview,
                context.prompt,
            );
        }
        let plan = match adapter.build_plan(LivenessRequest {
            api_key: provider.auth.api_key.trim(),
            base_url: &context.base_url,
            model: &context.model,
            prompt: &context.prompt,
            timeout_seconds: context.timeout_seconds,
            isolated_home: &isolated_home,
            output_path: &output_path,
        }) {
            Ok(plan) => plan,
            Err(message) => {
                cleanup_liveness_home(&isolated_home);
                return failure_record(
                    checked_at,
                    source,
                    message,
                    context.command_preview,
                    context.prompt,
                );
            }
        };
        if let Err(message) = write_plan_files(&isolated_home, &plan.files) {
            cleanup_liveness_home(&isolated_home);
            return failure_record(
                checked_at,
                source,
                message,
                context.command_preview,
                context.prompt,
            );
        }

        let mut command = Command::new(&context.cli_path);
        if let Some(path_env) = agent_cli::runtime_path_for(Path::new(&context.cli_path)) {
            command.env("PATH", path_env);
        }
        apply_common_isolated_home(&mut command, &isolated_home);
        apply_environment_patch(&mut command, &plan.environment);
        command.args(&plan.args);
        let proxy = network::resolve_proxy(settings, provider);
        network::apply_proxy_env(&mut command, &proxy);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        process::configure_process_group(&mut command);

        let started_at = Instant::now();
        let child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                cleanup_liveness_home(&isolated_home);
                return failure_record(
                    checked_at,
                    source,
                    format!("启动测活 CLI 失败: {err}"),
                    context.command_preview,
                    context.prompt,
                );
            }
        };
        let outcome = wait_with_output_timeout(child, Duration::from_secs(context.timeout_seconds));
        let latency_ms = started_at.elapsed().as_millis();
        let response_output = read_response_output(&plan.response_source, &outcome.stdout);
        cleanup_liveness_home(&isolated_home);

        if outcome.timed_out {
            return LivenessRecord {
                checked_at,
                source,
                cli_kind: context.cli_kind.key().to_string(),
                ok: false,
                latency_ms,
                model: context.model,
                base_url: context.base_url,
                prompt: context.prompt,
                response_preview: String::new(),
                response_raw: sanitize_output(&format!("{}\n{}", outcome.stderr, outcome.stdout)),
                input_tokens: None,
                cached_input_tokens: None,
                output_tokens: None,
                reasoning_output_tokens: None,
                total_tokens: None,
                total_cost_usd: None,
                message: format!("测活超时（{} 秒）", context.timeout_seconds),
                command_preview: context.command_preview,
            };
        }

        let exit_ok = outcome.status.is_some_and(|status| status.success());
        let parsed = adapter.parse_output(&response_output, &outcome.stdout);
        let response_raw = sanitize_output(&response_output);
        let message = if exit_ok && !parsed.response.is_empty() {
            "测活成功".to_string()
        } else {
            let detail = parsed.error.unwrap_or_else(|| {
                sanitize_output(&format!("{}\n{}", outcome.stderr, outcome.stdout))
            });
            if detail.trim().is_empty() {
                format!(
                    "测活 CLI 退出码异常: {:?}",
                    outcome.status.map(|status| status.code())
                )
            } else {
                detail
            }
        };

        LivenessRecord {
            checked_at,
            source,
            cli_kind: context.cli_kind.key().to_string(),
            ok: exit_ok && !parsed.response.is_empty(),
            latency_ms,
            model: context.model,
            base_url: context.base_url,
            prompt: context.prompt,
            response_preview: parsed.response,
            response_raw: if response_raw.trim().is_empty() {
                sanitize_output(&format!("{}\n{}", outcome.stderr, outcome.stdout))
            } else {
                response_raw
            },
            input_tokens: parsed.usage.input_tokens,
            cached_input_tokens: parsed.usage.cached_input_tokens,
            output_tokens: parsed.usage.output_tokens,
            reasoning_output_tokens: parsed.usage.reasoning_output_tokens,
            total_tokens: parsed.usage.total_tokens,
            total_cost_usd: parsed.usage.total_cost_usd,
            message,
            command_preview: context.command_preview,
        }
    }
}

fn unique_liveness_home(provider: &Provider, kind: AgentCliKind) -> PathBuf {
    env::temp_dir().join(format!(
        "balancehub-agent-liveness-{}-{}-{}-{}",
        kind.key(),
        provider.identity.id.replace('/', "_"),
        std::process::id(),
        now_millis()
    ))
}

fn write_plan_files(
    isolated_home: &Path,
    files: &[agent_cli::contracts::AgentFilePlan],
) -> Result<(), String> {
    for file in files {
        if !file.path.starts_with(isolated_home) {
            return Err("Agent CLI 测活文件超出隔离目录".to_string());
        }
        if let Some(parent) = file.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("创建测活 CLI 配置目录失败: {err}"))?;
        }
        fs::write(&file.path, &file.content)
            .map_err(|err| format!("写入测活 CLI 配置失败: {err}"))?;
    }
    Ok(())
}

fn apply_common_isolated_home(command: &mut Command, path: &Path) {
    command
        .env("HOME", path)
        .env("USERPROFILE", path)
        .env("APPDATA", path.join("AppData/Roaming"))
        .env("LOCALAPPDATA", path.join("AppData/Local"))
        .env("XDG_CONFIG_HOME", path.join(".config"))
        .env("XDG_CACHE_HOME", path.join(".cache"))
        .env("XDG_DATA_HOME", path.join(".local/share"));
}

fn apply_environment_patch(command: &mut Command, environment: &EnvironmentPatch) {
    for name in environment.removed_names() {
        command.env_remove(name);
    }
    for (name, value) in environment.set_values() {
        command.env(name, value);
    }
}

fn read_response_output(source: &LivenessResponseSource, stdout: &str) -> String {
    match source {
        LivenessResponseSource::Stdout => stdout.to_string(),
        LivenessResponseSource::File(path) => crate::util::read_text_file_limited(
            path,
            limits::MAX_LIVENESS_OUTPUT_FILE_BYTES,
            "读取测活结果",
        )
        .unwrap_or_default(),
    }
}

fn cleanup_liveness_home(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

fn format_command_preview(
    cli_path: &str,
    environment: &EnvironmentPatch,
    args: &[String],
) -> String {
    let mut parts = environment
        .set_values()
        .map(|(name, value)| format!("{name}={}", shell_quote(value)))
        .collect::<Vec<_>>();
    parts.push(shell_quote(cli_path));
    parts.extend(args.iter().map(|arg| shell_quote(arg)));
    parts.join(" ")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn failure_record(
    checked_at: String,
    source: String,
    message: String,
    command_preview: String,
    prompt: String,
) -> LivenessRecord {
    LivenessRecord {
        checked_at,
        source,
        cli_kind: String::new(),
        ok: false,
        latency_ms: 0,
        model: String::new(),
        base_url: String::new(),
        prompt,
        response_preview: String::new(),
        response_raw: String::new(),
        input_tokens: None,
        cached_input_tokens: None,
        output_tokens: None,
        reasoning_output_tokens: None,
        total_tokens: None,
        total_cost_usd: None,
        message,
        command_preview,
    }
}

fn sanitize_output(value: &str) -> String {
    value
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(1000)
        .collect()
}
