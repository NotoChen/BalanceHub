use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionTokenCounts {
    /// Input includes cache reads/writes; reasoning is part of output.
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsageRecord {
    pub id: String,
    /// Snapshot-based logs associate usage with a replaceable/rewindable message.
    pub message_id: Option<String>,
    pub model: Option<String>,
    pub tokens: SessionTokenCounts,
    pub cumulative: bool,
    pub uncertain: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionModelUsage {
    pub model: Option<String>,
    pub tokens: SessionTokenCounts,
    pub records: u64,
    pub estimated_cost_microusd: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionUsageSummary {
    pub tokens: SessionTokenCounts,
    pub models: Vec<SessionModelUsage>,
    pub partial: bool,
    pub native_cumulative: bool,
    pub records: u64,
    pub estimated_cost_microusd: Option<u64>,
    pub unpriced_records: u64,
}

impl SessionUsageSummary {
    /// 合并多个会话或分组的统计。token 字段沿用 `add` 的未知语义：
    /// 第一次写入复制实际值，之后只累加双方都已知的字段。费用只累加已定价
    /// 部分，未定价记录数保留，界面继续显示“已知部分参考估算”。
    pub fn merge(&mut self, other: &Self) {
        let first = self.records == 0;
        self.tokens.add(&other.tokens, first);
        self.partial |= other.partial;
        self.native_cumulative |= other.native_cumulative;
        self.records = self.records.saturating_add(other.records);
        self.unpriced_records = self.unpriced_records.saturating_add(other.unpriced_records);
        self.estimated_cost_microusd =
            sum_known_cost(self.estimated_cost_microusd, other.estimated_cost_microusd);
        for model in &other.models {
            let entry = match self
                .models
                .iter_mut()
                .find(|current| current.model == model.model)
            {
                Some(current) => current,
                None => {
                    self.models.push(model.clone());
                    continue;
                }
            };
            let first = entry.records == 0;
            entry.tokens.add(&model.tokens, first);
            entry.records = entry.records.saturating_add(model.records);
            entry.estimated_cost_microusd =
                sum_known_cost(entry.estimated_cost_microusd, model.estimated_cost_microusd);
        }
    }
}

fn sum_known_cost(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (None, None) => None,
        (left, right) => Some(
            left.unwrap_or_default()
                .saturating_add(right.unwrap_or_default()),
        ),
    }
}

impl SessionTokenCounts {
    /// 原生 usage 把缓存读写单独计数时（Anthropic 风格），落库前把缓存
    /// 并入 `input_tokens`，维持“输入含缓存”的统计约定。漏掉这一步会让
    /// 总 token 少算缓存，费用估算也会因“输入 - 缓存”下溢而放弃整条记录。
    /// 输入本身未知时保持未知，不用缓存值伪造总量。
    pub fn with_cache_in_input(mut self) -> Self {
        if let Some(input) = self.input_tokens {
            self.input_tokens = Some(
                input
                    .saturating_add(self.cache_read_tokens.unwrap_or_default())
                    .saturating_add(self.cache_write_tokens.unwrap_or_default()),
            );
        }
        self
    }

    /// 原生 usage 把推理从输出中拆开时，落库前并入 `output_tokens`，维持
    /// “输出含推理”的统计约定；费用估算只按输出计价，漏掉会少算推理。
    /// 输出本身未知时保持未知。推理已经是输出的子集时不要调用。
    pub fn with_reasoning_in_output(mut self) -> Self {
        if let Some(output) = self.output_tokens {
            self.output_tokens =
                Some(output.saturating_add(self.reasoning_tokens.unwrap_or_default()));
        }
        self
    }

    pub fn add(&mut self, other: &Self, first: bool) {
        for (target, value) in [
            (&mut self.input_tokens, other.input_tokens),
            (&mut self.output_tokens, other.output_tokens),
            (&mut self.cache_read_tokens, other.cache_read_tokens),
            (&mut self.cache_write_tokens, other.cache_write_tokens),
            (&mut self.reasoning_tokens, other.reasoning_tokens),
        ] {
            *target = if first {
                value
            } else {
                target
                    .zip(value)
                    .map(|(total, value)| total.saturating_add(value))
            };
        }
    }
    pub fn retain_maximum(&mut self, previous: &Self) -> bool {
        let mut decreased = false;
        for (target, old) in [
            (&mut self.input_tokens, previous.input_tokens),
            (&mut self.output_tokens, previous.output_tokens),
            (&mut self.cache_read_tokens, previous.cache_read_tokens),
            (&mut self.cache_write_tokens, previous.cache_write_tokens),
            (&mut self.reasoning_tokens, previous.reasoning_tokens),
        ] {
            if let Some(old) = old {
                decreased |= target.is_some_and(|current| current < old);
                *target = Some(target.unwrap_or_default().max(old));
            }
        }
        decreased
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(
        records: u64,
        input: Option<u64>,
        output: Option<u64>,
        cost: Option<u64>,
    ) -> SessionUsageSummary {
        SessionUsageSummary {
            tokens: SessionTokenCounts {
                input_tokens: input,
                output_tokens: output,
                ..SessionTokenCounts::default()
            },
            models: vec![SessionModelUsage {
                model: Some("m".into()),
                tokens: SessionTokenCounts {
                    input_tokens: input,
                    output_tokens: output,
                    ..SessionTokenCounts::default()
                },
                records,
                estimated_cost_microusd: cost,
            }],
            records,
            estimated_cost_microusd: cost,
            unpriced_records: if cost.is_none() { records } else { 0 },
            ..SessionUsageSummary::default()
        }
    }

    #[test]
    fn merge_sums_known_tokens_and_keeps_unknown_fields_unknown() {
        let mut total = summary(1, Some(100), Some(10), Some(5));
        total.merge(&summary(2, Some(50), None, None));
        assert_eq!(total.records, 3);
        assert_eq!(total.tokens.input_tokens, Some(150));
        assert_eq!(total.tokens.output_tokens, None);
        assert_eq!(total.models.len(), 1);
        assert_eq!(total.models[0].records, 3);
        assert_eq!(total.models[0].tokens.input_tokens, Some(150));
        assert_eq!(total.estimated_cost_microusd, Some(5));
        assert_eq!(total.unpriced_records, 2);
    }

    #[test]
    fn merging_into_an_empty_summary_copies_the_first_values() {
        let mut total = SessionUsageSummary::default();
        total.merge(&summary(2, Some(50), Some(5), Some(3)));
        total.merge(&summary(1, Some(25), Some(5), Some(1)));
        assert_eq!(total.tokens.input_tokens, Some(75));
        assert_eq!(total.tokens.output_tokens, Some(10));
        assert_eq!(total.estimated_cost_microusd, Some(4));
        assert!(!total.partial);
    }

    #[test]
    fn including_cache_folds_separate_cache_counts_into_input() {
        let counts = SessionTokenCounts {
            input_tokens: Some(120),
            cache_read_tokens: Some(40),
            cache_write_tokens: Some(10),
            ..SessionTokenCounts::default()
        }
        .with_cache_in_input();
        assert_eq!(counts.input_tokens, Some(170));
        // 缓存字段保留，费用估算仍按缓存价单独计价。
        assert_eq!(counts.cache_read_tokens, Some(40));
        assert_eq!(counts.cache_write_tokens, Some(10));
    }

    #[test]
    fn with_reasoning_in_output_folds_separate_reasoning_into_output() {
        let counts = SessionTokenCounts {
            output_tokens: Some(30),
            reasoning_tokens: Some(12),
            ..SessionTokenCounts::default()
        }
        .with_reasoning_in_output();
        assert_eq!(counts.output_tokens, Some(42));
        // 推理字段保留，供界面单独展示明细。
        assert_eq!(counts.reasoning_tokens, Some(12));
    }

    #[test]
    fn with_reasoning_in_output_keeps_unknown_output_unknown() {
        let counts = SessionTokenCounts {
            reasoning_tokens: Some(12),
            ..SessionTokenCounts::default()
        }
        .with_reasoning_in_output();
        assert_eq!(counts.output_tokens, None);
    }

    #[test]
    fn including_cache_keeps_unknown_input_unknown() {
        let counts = SessionTokenCounts {
            cache_read_tokens: Some(40),
            ..SessionTokenCounts::default()
        }
        .with_cache_in_input();
        assert_eq!(counts.input_tokens, None);
    }
}
