//! 站点观测到的模型运行表现。
//!
//! NewAPI 的 `/api/perf-metrics/summary` 按模型汇总最近窗口内的请求成功率、
//! 平均延迟和吞吐。它描述的是站点真实流量，不是本机测活结果，因此单独保存，
//! 不和价格、模型列表混在一起。读取失败时保留上一次成功读数，避免一次网络
//! 抖动让模型广场突然失去可用性信息。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderPerformancePoint {
    /// unix 秒。
    pub ts: i64,
    /// 成功率百分比（0–100）。
    pub success_rate: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderModelPerformance {
    /// 成功率百分比（0–100）。
    pub success_rate: Option<f64>,
    pub avg_latency_ms: Option<f64>,
    /// 输出 token / 秒。
    pub avg_tps: Option<f64>,
    /// 近期成功率采样；站点未返回时为空。
    pub recent_success: Vec<ProviderPerformancePoint>,
}

impl ProviderModelPerformance {
    pub fn has_metrics(&self) -> bool {
        self.success_rate.is_some()
            || self.avg_latency_ms.is_some()
            || self.avg_tps.is_some()
            || !self.recent_success.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderPerformanceSnapshot {
    pub models: BTreeMap<String, ProviderModelPerformance>,
    /// 统计窗口的起止时间（unix 秒）。
    pub window_start: Option<i64>,
    pub window_end: Option<i64>,
    /// unix 毫秒字符串，与模型列表状态保持一致。
    pub updated_at: Option<String>,
    pub error: Option<String>,
}

impl ProviderPerformanceSnapshot {
    pub fn model(&self, model: &str) -> Option<&ProviderModelPerformance> {
        self.models.get(model)
    }

    /// 成功读取空性能数据也是一次有效结论：站点可能还没有流量。
    pub fn mark_loaded(
        &mut self,
        models: BTreeMap<String, ProviderModelPerformance>,
        window_start: Option<i64>,
        window_end: Option<i64>,
    ) {
        self.models = models;
        self.window_start = window_start;
        self.window_end = window_end;
        self.updated_at = Some(crate::util::unix_millis().to_string());
        self.error = None;
    }

    pub fn mark_error(&mut self, error: String) {
        self.error = Some(error);
    }

    /// 站点明确没有性能接口时清空旧数据，避免继续展示已经失效的观测结果。
    pub fn mark_unsupported(&mut self, error: String) {
        self.models.clear();
        self.window_start = None;
        self.window_end = None;
        self.updated_at = None;
        self.error = Some(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded() -> ProviderPerformanceSnapshot {
        let mut snapshot = ProviderPerformanceSnapshot::default();
        snapshot.mark_loaded(
            BTreeMap::from([(
                "model-a".to_string(),
                ProviderModelPerformance {
                    success_rate: Some(99.5),
                    avg_tps: Some(42.0),
                    ..ProviderModelPerformance::default()
                },
            )]),
            Some(1),
            Some(2),
        );
        snapshot
    }

    #[test]
    fn transient_error_keeps_the_last_successful_metrics() {
        let mut snapshot = loaded();
        snapshot.mark_error("网络超时".to_string());
        assert!(snapshot.model("model-a").is_some());
        assert_eq!(snapshot.window_start, Some(1));
        assert_eq!(snapshot.error.as_deref(), Some("网络超时"));
    }

    #[test]
    fn confirmed_unsupported_performance_clears_stale_metrics() {
        let mut snapshot = loaded();
        snapshot.mark_unsupported("站点未提供性能接口".to_string());
        assert!(snapshot.models.is_empty());
        assert!(snapshot.updated_at.is_none());
        assert_eq!(snapshot.error.as_deref(), Some("站点未提供性能接口"));
    }

    #[test]
    fn success_samples_are_metrics_on_their_own() {
        assert!(ProviderModelPerformance {
            recent_success: vec![ProviderPerformancePoint {
                ts: 1,
                success_rate: 100.0,
            }],
            ..ProviderModelPerformance::default()
        }
        .has_metrics());
    }
}
