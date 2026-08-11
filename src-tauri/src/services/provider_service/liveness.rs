use crate::{
    limits,
    models::{
        CliEnvironmentProbeResult, CliToolProbeResult, CodexCliProbeResult, LivenessCliKind,
        LivenessPromptMode, LivenessRecord, TerminalEnvironmentProbeResult,
    },
    services::liveness::{effective_interval, LivenessRunner},
    util::unix_millis as current_timestamp_millis,
};

use super::{find_provider, ProviderRequestContext, ProviderService};

impl<'a> ProviderService<'a> {
    pub fn probe_cli_tools(
        &self,
        include_shell: bool,
    ) -> Result<CliEnvironmentProbeResult, String> {
        let snapshot = self.snapshot();
        let codex_path = snapshot.settings.codex_cli_path.clone();
        let claude_path = snapshot.settings.claude_cli_path.clone();
        let (codex, claude_code) = std::thread::scope(|scope| {
            let codex_handle = scope.spawn(|| {
                if include_shell {
                    LivenessRunner::find_codex_cli(&codex_path)
                } else {
                    LivenessRunner::find_codex_cli_without_shell(&codex_path)
                }
            });
            let claude_handle = scope.spawn(|| {
                if include_shell {
                    LivenessRunner::find_claude_cli(&claude_path)
                } else {
                    LivenessRunner::find_claude_cli_without_shell(&claude_path)
                }
            });

            let codex = codex_handle
                .join()
                .unwrap_or_else(|_| Err("Codex CLI 自动检测异常".to_string()));
            let claude = claude_handle
                .join()
                .unwrap_or_else(|_| Err("Claude Code CLI 自动检测异常".to_string()));

            (
                cli_tool_probe_result(LivenessCliKind::Codex, codex),
                cli_tool_probe_result(LivenessCliKind::ClaudeCode, claude),
            )
        });

        Ok(CliEnvironmentProbeResult { codex, claude_code })
    }

    pub fn probe_terminals(&self) -> TerminalEnvironmentProbeResult {
        TerminalEnvironmentProbeResult {
            terminals: crate::services::temporary_cli::probe_available_terminals(),
        }
    }

    pub fn run_liveness(
        &self,
        id: String,
        prompt: Option<String>,
        automatic: bool,
    ) -> Result<LivenessRecord, String> {
        let snapshot = self.snapshot();
        let provider = find_provider(&snapshot, &id)?;
        if !provider.runtime.enabled {
            return Err("中转站已停用".to_string());
        }
        let request_context = ProviderRequestContext::capture(&provider);

        let record = LivenessRunner::run(&snapshot.settings, &provider, prompt, automatic);
        let stored_record = record.clone();
        // 累计统计独立于明细记录上限，每次自动测活都计入实际消耗。
        let run_input_tokens = record.input_tokens.unwrap_or(0);
        let run_output_tokens = record.output_tokens.unwrap_or(0);
        let run_total_tokens = record.total_tokens.unwrap_or(0);
        let run_cost_usd = record.total_cost_usd.unwrap_or(0.0);
        self.mutate(|data| {
            if let Some(stored_provider) = data
                .providers
                .iter_mut()
                .find(|stored| stored.identity.id == id && request_context.matches(stored))
            {
                stored_provider.liveness.records.push(stored_record);
                stored_provider.liveness.run_count =
                    stored_provider.liveness.run_count.saturating_add(1);
                stored_provider.liveness.total_input_tokens = stored_provider
                    .liveness
                    .total_input_tokens
                    .saturating_add(run_input_tokens);
                stored_provider.liveness.total_output_tokens = stored_provider
                    .liveness
                    .total_output_tokens
                    .saturating_add(run_output_tokens);
                stored_provider.liveness.total_tokens = stored_provider
                    .liveness
                    .total_tokens
                    .saturating_add(run_total_tokens);
                stored_provider.liveness.total_cost_usd += run_cost_usd;
                if stored_provider.liveness.records.len() > limits::MAX_LIVENESS_RECORDS {
                    let remove_count =
                        stored_provider.liveness.records.len() - limits::MAX_LIVENESS_RECORDS;
                    stored_provider.liveness.records.drain(0..remove_count);
                }
                if matches!(
                    if stored_provider.liveness.use_global {
                        data.settings.liveness_prompt_mode
                    } else {
                        stored_provider.liveness.prompt_mode
                    },
                    LivenessPromptMode::RoundRobin
                ) {
                    stored_provider.liveness.prompt_cursor =
                        stored_provider.liveness.prompt_cursor.saturating_add(1);
                }
                let next_after = effective_interval(&data.settings, stored_provider);
                stored_provider.liveness.next_at =
                    Some((current_timestamp_millis() + next_after as u128 * 1000).to_string());
            }
            // 注意：mutate 闭包持有状态写锁，严禁在这里做磁盘扫描/子进程探测之类的
            // 阻塞操作。CLI 探测由独立命令（锁外）负责。
        })?;

        Ok(record)
    }
}

fn cli_tool_probe_result(
    cli_kind: LivenessCliKind,
    result: Result<CodexCliProbeResult, String>,
) -> CliToolProbeResult {
    match result {
        Ok(result) => CliToolProbeResult {
            available: true,
            path: result.path,
            version: result.version,
            message: String::new(),
            supports_session_name: cli_kind.supports_session_name(),
        },
        Err(message) => CliToolProbeResult {
            available: false,
            path: String::new(),
            version: String::new(),
            message,
            supports_session_name: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::cli_tool_probe_result;
    use crate::models::{CodexCliProbeResult, LivenessCliKind};

    #[test]
    fn cli_probe_exposes_launch_naming_capability_from_rust() {
        let claude = cli_tool_probe_result(
            LivenessCliKind::ClaudeCode,
            Ok(CodexCliProbeResult {
                path: "/usr/local/bin/claude".to_string(),
                version: "2.1.221".to_string(),
            }),
        );
        let codex = cli_tool_probe_result(
            LivenessCliKind::Codex,
            Ok(CodexCliProbeResult {
                path: "/usr/local/bin/codex".to_string(),
                version: "0.146.0".to_string(),
            }),
        );

        assert!(claude.supports_session_name);
        assert!(!codex.supports_session_name);
    }
}
