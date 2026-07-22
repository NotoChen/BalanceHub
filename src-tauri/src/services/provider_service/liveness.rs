use crate::{
    models::{
        CliEnvironmentProbeResult, CliToolProbeResult, CodexCliProbeResult, LivenessPromptMode,
        LivenessRecord, TemporaryCliTerminalKind,
    },
    services::{
        liveness::{effective_interval, LivenessRunner},
        temporary_cli,
    },
    util::unix_millis as current_timestamp_millis,
};

use super::{find_provider, ProviderService};

impl<'a> ProviderService<'a> {
    pub fn probe_cli_environment(
        &self,
        terminal_kind: Option<TemporaryCliTerminalKind>,
        terminal_command: Option<String>,
    ) -> Result<CliEnvironmentProbeResult, String> {
        let snapshot = self.snapshot();
        let codex_path = snapshot.settings.codex_cli_path.clone();
        let claude_path = snapshot.settings.claude_cli_path.clone();
        let terminal_kind = terminal_kind.unwrap_or(snapshot.settings.temporary_cli_terminal_kind);
        let terminal_command = terminal_command
            .unwrap_or(snapshot.settings.temporary_cli_terminal_command)
            .trim()
            .to_string();

        let (codex, claude_code, terminal) = std::thread::scope(|scope| {
            let codex_handle = scope.spawn(|| LivenessRunner::find_codex_cli(&codex_path));
            let claude_handle = scope.spawn(|| LivenessRunner::find_claude_cli(&claude_path));
            let terminal_handle =
                scope.spawn(|| temporary_cli::probe_terminal(terminal_kind, &terminal_command));

            let codex = codex_handle
                .join()
                .unwrap_or_else(|_| Err("Codex CLI 自动检测异常".to_string()));
            let claude = claude_handle
                .join()
                .unwrap_or_else(|_| Err("Claude Code CLI 自动检测异常".to_string()));
            let terminal = terminal_handle.join().unwrap_or_else(|_| {
                crate::models::TemporaryTerminalProbeResult {
                    available: false,
                    kind: terminal_kind,
                    name: "临时终端".to_string(),
                    version: String::new(),
                    message: "临时终端自动检测异常".to_string(),
                }
            });

            (
                cli_tool_probe_result(codex),
                cli_tool_probe_result(claude),
                terminal,
            )
        });

        let stored_codex_path = codex.path.clone();
        let stored_claude_path = claude_code.path.clone();
        self.mutate(|data| {
            data.settings.codex_cli_path = stored_codex_path;
            data.settings.claude_cli_path = stored_claude_path;
        })?;

        Ok(CliEnvironmentProbeResult {
            codex,
            claude_code,
            terminal,
        })
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

        let record = LivenessRunner::run(&snapshot.settings, &provider, prompt, automatic);
        let stored_record = record.clone();
        // 累计统计独立于 40 条记录上限，每次自动测活都计入实际消耗。
        let run_input_tokens = record.input_tokens.unwrap_or(0);
        let run_output_tokens = record.output_tokens.unwrap_or(0);
        let run_total_tokens = record.total_tokens.unwrap_or(0);
        let run_cost_usd = record.total_cost_usd.unwrap_or(0.0);
        self.mutate(|data| {
            if let Some(stored_provider) = data
                .providers
                .iter_mut()
                .find(|stored| stored.identity.id == id)
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
                if stored_provider.liveness.records.len() > 40 {
                    let remove_count = stored_provider.liveness.records.len() - 40;
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
            // 阻塞操作。CLI 路径的自动发现由启动时的 probe_cli_environment（锁外）负责。
        })?;

        Ok(record)
    }
}

fn cli_tool_probe_result(result: Result<CodexCliProbeResult, String>) -> CliToolProbeResult {
    match result {
        Ok(result) => CliToolProbeResult {
            available: true,
            path: result.path,
            version: result.version,
            message: String::new(),
        },
        Err(message) => CliToolProbeResult {
            available: false,
            path: String::new(),
            version: String::new(),
            message,
        },
    }
}
