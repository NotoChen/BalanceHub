use crate::{
    limits,
    models::{LivenessPromptMode, LivenessRecord},
    services::liveness::{effective_interval, LivenessRunner},
    util::unix_millis as current_timestamp_millis,
};

use super::{find_provider, MutationDecision, ProviderRequestContext, ProviderService};

impl<'a> ProviderService<'a> {
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
        let persisted = self.mutate_decided(|data| {
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
                return Ok(MutationDecision::changed(true));
            }
            // 注意：事务闭包持有 mutation gate，严禁在这里做磁盘扫描/子进程探测。
            // CLI 探测由独立命令在锁外完成。
            Ok(MutationDecision::unchanged(false))
        })?;

        if !persisted {
            return Err("本地配置已变更，本次测活结果已忽略".to_string());
        }

        Ok(record)
    }
}
