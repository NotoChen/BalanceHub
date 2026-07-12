use crate::{
    models::{
        AppSettings, CliCandidate, CodexCliProbeResult, LivenessCliKind, LivenessPromptMode,
        LivenessRecord, LivenessRunResult,
    },
    services::liveness::{effective_interval, LivenessRunner},
    util::unix_millis as current_timestamp_millis,
};

use super::{find_provider, ProviderService};

impl<'a> ProviderService<'a> {
    pub fn probe_codex_cli(
        &self,
        liveness_cli_kind: Option<LivenessCliKind>,
        codex_cli_path: Option<String>,
        claude_cli_path: Option<String>,
    ) -> Result<CodexCliProbeResult, String> {
        let mut settings = self.snapshot();
        if let Some(kind) = liveness_cli_kind {
            settings.settings.liveness_cli_kind = kind;
        }
        if let Some(path) = codex_cli_path {
            settings.settings.codex_cli_path = path;
        }
        if let Some(path) = claude_cli_path {
            settings.settings.claude_cli_path = path;
        }
        if settings.settings.codex_cli_path.trim().is_empty() {
            if let Ok(result) = LivenessRunner::find_codex_cli("") {
                settings.settings.codex_cli_path = result.path;
            }
        }
        if settings.settings.claude_cli_path.trim().is_empty() {
            if let Ok(result) = LivenessRunner::find_claude_cli("") {
                settings.settings.claude_cli_path = result.path;
            }
        }
        let result = match settings.settings.liveness_cli_kind {
            LivenessCliKind::Codex => {
                let result = LivenessRunner::find_codex_cli(&settings.settings.codex_cli_path)?;
                settings.settings.codex_cli_path = result.path.clone();
                result
            }
            LivenessCliKind::ClaudeCode => {
                let result = LivenessRunner::find_claude_cli(&settings.settings.claude_cli_path)?;
                settings.settings.claude_cli_path = result.path.clone();
                result
            }
        };

        let codex_path = settings.settings.codex_cli_path.clone();
        let claude_path = settings.settings.claude_cli_path.clone();
        self.mutate(|data| {
            data.settings.codex_cli_path = codex_path;
            data.settings.claude_cli_path = claude_path;
        })?;
        Ok(result)
    }

    pub fn liveness_command_preview(&self, id: String) -> Result<LivenessRunResult, String> {
        let data = self.snapshot();
        let provider = find_provider(&data, &id)?;
        let context = LivenessRunner::build_context(&data.settings, &provider, None)?;
        let record = LivenessRecord {
            checked_at: current_timestamp_millis().to_string(),
            source: "manual".to_string(),
            cli_kind: match context.cli_kind {
                LivenessCliKind::Codex => "codex".to_string(),
                LivenessCliKind::ClaudeCode => "claudeCode".to_string(),
            },
            ok: true,
            latency_ms: 0,
            model: context.model,
            base_url: context.base_url,
            prompt: context.prompt,
            response_preview: String::new(),
            response_raw: String::new(),
            input_tokens: None,
            cached_input_tokens: None,
            output_tokens: None,
            reasoning_output_tokens: None,
            total_tokens: None,
            total_cost_usd: None,
            message: "命令预览".to_string(),
            command_preview: context.command_preview,
        };
        Ok(LivenessRunResult {
            providers: data.providers,
            provider,
            record,
            codex_path: context.cli_path,
        })
    }

    pub fn test_liveness(
        &self,
        id: String,
        prompt: Option<String>,
        automatic: bool,
    ) -> Result<LivenessRunResult, String> {
        let snapshot = self.snapshot();
        let provider = find_provider(&snapshot, &id)?;
        if !provider.runtime.enabled {
            return Err("中转站已停用".to_string());
        }

        let record = LivenessRunner::run(&snapshot.settings, &provider, prompt, automatic);
        let codex_path = match record.cli_kind.as_str() {
            "claudeCode" => snapshot.settings.claude_cli_path.clone(),
            _ => snapshot.settings.codex_cli_path.clone(),
        };

        let stored_record = record.clone();
        // 累计统计（独立于 40 条记录上限）；手动与自动测活都计入实际消耗。
        let run_input_tokens = record.input_tokens.unwrap_or(0);
        let run_output_tokens = record.output_tokens.unwrap_or(0);
        let run_total_tokens = record.total_tokens.unwrap_or(0);
        let run_cost_usd = record.total_cost_usd.unwrap_or(0.0);
        let (providers, updated_provider) = self.mutate(|data| {
            let mut updated_provider = provider.clone();
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
                updated_provider = stored_provider.clone();
            }
            // 注意：mutate 闭包持有状态写锁，严禁在这里做磁盘扫描/子进程探测之类的
            // 阻塞操作。CLI 路径的自动发现由启动时的 probe_codex_cli（锁外）负责。
            (data.providers.clone(), updated_provider)
        })?;

        Ok(LivenessRunResult {
            providers,
            provider: updated_provider,
            record,
            codex_path,
        })
    }

    /// 记录全 App 自动测活（消耗真实额度）的一次性授权。
    pub fn acknowledge_liveness_cost(&self) -> Result<AppSettings, String> {
        let accepted_at = current_timestamp_millis().to_string();
        self.mutate(|data| {
            data.settings.liveness_consent_accepted_at = Some(accepted_at);
            data.settings.clone()
        })
    }

    /// 重置全 App 自动测活授权（重置后调度器停止自动测活，再次开启时会重新弹窗授权；手动测活不受影响）。
    pub fn revoke_liveness_cost(&self) -> Result<AppSettings, String> {
        self.mutate(|data| {
            data.settings.liveness_consent_accepted_at = None;
            data.settings.clone()
        })
    }

    /// 校验/发现某种测活 CLI：传入路径则校验该路径（解析 + `--version`），
    /// 传空则按内置规则自动发现。返回解析到的可执行路径与版本，或失败原因。不落盘。
    pub fn check_cli_path(
        &self,
        kind: LivenessCliKind,
        path: &str,
    ) -> Result<CodexCliProbeResult, String> {
        match kind {
            LivenessCliKind::Codex => LivenessRunner::find_codex_cli(path),
            LivenessCliKind::ClaudeCode => LivenessRunner::find_claude_cli(path),
        }
    }

    /// 枚举某种测活 CLI 的所有候选可执行文件（含来源/版本/有效性），供「显示所有候选」列表。
    pub fn list_cli_candidates(&self, kind: LivenessCliKind, path: &str) -> Vec<CliCandidate> {
        match kind {
            LivenessCliKind::Codex => LivenessRunner::enumerate_codex_cli(path),
            LivenessCliKind::ClaudeCode => LivenessRunner::enumerate_claude_cli(path),
        }
    }
}
