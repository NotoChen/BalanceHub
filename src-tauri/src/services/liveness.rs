mod cli;
mod command;
mod output;
mod process;
mod prompt;

pub use prompt::{anthropic_base_url, effective_interval, openai_base_url, preview_prompts};

use crate::{
    models::{
        provider_domain, AppSettings, CliCandidate, CodexCliProbeResult, LivenessCliKind,
        LivenessRecord, Provider,
    },
    network,
    util::unix_millis as now_millis,
};
use command::{
    apply_claude_isolated_home, apply_codex_isolated_home, build_claude_command,
    build_codex_command, build_command_preview, unique_claude_home_path, unique_codex_home_path,
    unique_output_path,
};
use output::{
    cli_kind_value, failure_record, parse_liveness_output, parse_token_usage, sanitize_output,
};
use process::wait_with_output_timeout;
use prompt::{effective_cli_kind, effective_model, effective_timeout, select_prompt};
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub struct LivenessRunner;

#[derive(Debug, Clone)]
pub struct LivenessContext {
    pub cli_kind: LivenessCliKind,
    pub cli_path: String,
    pub model: String,
    pub base_url: String,
    pub prompt: String,
    pub timeout_seconds: u64,
    pub command_preview: String,
}

impl LivenessRunner {
    pub fn find_codex_cli(preferred_path: &str) -> Result<CodexCliProbeResult, String> {
        cli::find_codex_cli(preferred_path)
    }

    pub fn find_claude_cli(preferred_path: &str) -> Result<CodexCliProbeResult, String> {
        cli::find_claude_cli(preferred_path)
    }

    pub fn enumerate_codex_cli(preferred_path: &str) -> Vec<CliCandidate> {
        cli::enumerate_codex_cli(preferred_path)
    }

    pub fn enumerate_claude_cli(preferred_path: &str) -> Vec<CliCandidate> {
        cli::enumerate_claude_cli(preferred_path)
    }

    /// 仅供非 Windows 的临时 CLI 脚本生成注入 PATH；Windows 启动脚本不导出 PATH。
    #[cfg(not(target_os = "windows"))]
    pub fn runtime_path_for_cli(cli_path: &std::path::Path) -> Option<std::ffi::OsString> {
        cli::runtime_path_for(cli_path)
    }

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
        let cli = match cli_kind {
            LivenessCliKind::Codex => Self::find_codex_cli(&settings.codex_cli_path)?,
            LivenessCliKind::ClaudeCode => Self::find_claude_cli(&settings.claude_cli_path)?,
        };
        let base_url = match cli_kind {
            LivenessCliKind::Codex => openai_base_url(provider),
            LivenessCliKind::ClaudeCode => anthropic_base_url(provider),
        };
        let command_preview =
            build_command_preview(cli_kind, &cli.path, &model, &base_url, &prompt);

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

        let isolated_home = match context.cli_kind {
            LivenessCliKind::Codex => Some(unique_codex_home_path(provider)),
            LivenessCliKind::ClaudeCode => Some(unique_claude_home_path(provider)),
        };
        if let Some(path) = &isolated_home {
            if let Err(err) = fs::create_dir_all(path) {
                return failure_record(
                    checked_at,
                    source,
                    format!("创建测活 CLI 临时目录失败: {err}"),
                    context.command_preview,
                    context.prompt,
                );
            }
        }

        let mut command = Command::new(&context.cli_path);
        if let Some(path_env) = cli::runtime_path_for(std::path::Path::new(&context.cli_path)) {
            command.env("PATH", path_env);
        }
        match context.cli_kind {
            LivenessCliKind::Codex => build_codex_command(&mut command, &context, provider),
            LivenessCliKind::ClaudeCode => build_claude_command(&mut command, &context, provider),
        }
        if let Some(path) = &isolated_home {
            match context.cli_kind {
                LivenessCliKind::Codex => apply_codex_isolated_home(&mut command, path),
                LivenessCliKind::ClaudeCode => apply_claude_isolated_home(&mut command, path),
            }
        }
        let proxy = network::resolve_proxy(settings, provider);
        network::apply_proxy_env(&mut command, &proxy);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // 独立进程组：超时才能连 CLI 拉起的 helper 子进程一起杀灭（见 process.rs）。
        process::configure_process_group(&mut command);

        let output_path = if matches!(context.cli_kind, LivenessCliKind::Codex) {
            Some(unique_output_path(provider))
        } else {
            None
        };
        if let Some(output_path) = &output_path {
            command.arg("-o").arg(output_path).arg(&context.prompt);
        }

        let started_at = Instant::now();
        let child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                cleanup_isolated_home(&isolated_home);
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
        cleanup_isolated_home(&isolated_home);
        let timed_out = outcome.timed_out;
        let status = outcome.status;
        let stdout = outcome.stdout;
        let stderr = outcome.stderr;

        let response = output_path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .unwrap_or_default();
        if let Some(output_path) = &output_path {
            let _ = fs::remove_file(output_path);
        }
        let latency_ms = started_at.elapsed().as_millis();

        if timed_out {
            return LivenessRecord {
                checked_at,
                source,
                cli_kind: cli_kind_value(context.cli_kind),
                ok: false,
                latency_ms,
                model: context.model,
                base_url: context.base_url,
                prompt: context.prompt,
                response_preview: String::new(),
                response_raw: sanitize_output(&format!("{stderr}\n{stdout}")),
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

        let exit_ok = status.is_some_and(|status| status.success());
        let raw_response = match context.cli_kind {
            LivenessCliKind::Codex => response,
            LivenessCliKind::ClaudeCode => stdout.clone(),
        };
        let token_usage = parse_token_usage(context.cli_kind, &stdout);
        let (response_preview, parsed_error) =
            parse_liveness_output(context.cli_kind, &raw_response);
        let response_raw = sanitize_output(&raw_response);
        let message = if exit_ok && !response_preview.is_empty() {
            "测活成功".to_string()
        } else {
            let detail =
                parsed_error.unwrap_or_else(|| sanitize_output(&format!("{stderr}\n{stdout}")));
            if detail.trim().is_empty() {
                format!(
                    "测活 CLI 退出码异常: {:?}",
                    status.map(|status| status.code())
                )
            } else {
                detail
            }
        };

        LivenessRecord {
            checked_at,
            source,
            cli_kind: cli_kind_value(context.cli_kind),
            ok: exit_ok && !response_preview.is_empty(),
            latency_ms,
            model: context.model,
            base_url: context.base_url,
            prompt: context.prompt,
            response_preview,
            response_raw: if response_raw.trim().is_empty() {
                sanitize_output(&format!("{stderr}\n{stdout}"))
            } else {
                response_raw
            },
            input_tokens: token_usage.input_tokens,
            cached_input_tokens: token_usage.cached_input_tokens,
            output_tokens: token_usage.output_tokens,
            reasoning_output_tokens: token_usage.reasoning_output_tokens,
            total_tokens: token_usage.total_tokens,
            total_cost_usd: token_usage.total_cost_usd,
            message,
            command_preview: context.command_preview,
        }
    }
}

fn cleanup_isolated_home(path: &Option<std::path::PathBuf>) {
    if let Some(path) = path {
        let _ = fs::remove_dir_all(path);
    }
}
