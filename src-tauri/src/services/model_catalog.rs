//! Optional descriptions from models.dev. This service never grants availability.
use crate::{models::*, network, services::agent_cli::cache};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_AGE: i64 = 24 * 60 * 60;

#[derive(Default, Serialize, Deserialize)]
struct Catalog {
    updated_at: i64,
    models: BTreeMap<String, ModelMetadata>,
}
pub(crate) struct ModelCatalogService {
    path: PathBuf,
    catalog: Mutex<Catalog>,
    gate: tokio::sync::Mutex<()>,
    attempt: Mutex<Option<(Instant, Option<String>)>>,
}
impl ModelCatalogService {
    pub(crate) fn estimate(
        &self,
        mut usage: SessionUsageSummary,
    ) -> Result<SessionUsageSummary, String> {
        let catalog = self.catalog.lock().map_err(|_| "模型资料暂不可用")?;
        usage.estimated_cost_microusd = None;
        usage.unpriced_records = 0;
        for model in &mut usage.models {
            model.estimated_cost_microusd = model
                .model
                .as_ref()
                .and_then(|id| catalog.models.get(id))
                .and_then(|metadata| estimate(&model.tokens, &metadata.price));
            if let Some(price) = model.estimated_cost_microusd {
                usage.estimated_cost_microusd = Some(
                    usage
                        .estimated_cost_microusd
                        .unwrap_or_default()
                        .saturating_add(price),
                );
            } else {
                usage.unpriced_records = usage.unpriced_records.saturating_add(model.records);
            }
        }
        Ok(usage)
    }
    pub(crate) fn new(path: PathBuf) -> Self {
        let catalog = std::fs::metadata(&path)
            .ok()
            .filter(|metadata| metadata.len() <= MAX_BYTES as u64)
            .and_then(|_| cache::read(&path))
            .unwrap_or_default();
        Self {
            path,
            catalog: Mutex::new(catalog),
            gate: tokio::sync::Mutex::new(()),
            attempt: Mutex::new(None),
        }
    }
    pub(crate) fn snapshot(&self, ids: &[String]) -> Result<ModelMetadataSnapshot, String> {
        let catalog = self.catalog.lock().map_err(|_| "模型资料暂不可用")?;
        let models = ids
            .iter()
            .filter_map(|id| catalog.models.get(id))
            .cloned()
            .collect();
        let updated_at = chrono::DateTime::from_timestamp(catalog.updated_at, 0)
            .filter(|_| catalog.updated_at > 0)
            .map(|date| date.to_rfc3339());
        let error = self
            .attempt
            .lock()
            .ok()
            .and_then(|attempt| attempt.as_ref().and_then(|(_, error)| error.clone()));
        Ok(ModelMetadataSnapshot {
            models,
            updated_at,
            stale: chrono::Utc::now()
                .timestamp()
                .saturating_sub(catalog.updated_at)
                >= MAX_AGE,
            error,
        })
    }
    pub(crate) async fn refresh(&self, settings: &AppSettings, force: bool) -> Result<(), String> {
        let requested = Instant::now();
        let _gate = self.gate.lock().await;
        if let Some((completed, error)) = self
            .attempt
            .lock()
            .map_err(|_| "模型资料暂不可用")?
            .as_ref()
        {
            // Explicit refreshes that arrived together share one result too.
            if *completed >= requested {
                return error.clone().map_or(Ok(()), Err);
            }
        }
        let fresh = self
            .catalog
            .lock()
            .map_err(|_| "模型资料暂不可用")?
            .updated_at
            > chrono::Utc::now().timestamp() - MAX_AGE;
        if fresh && !force {
            return Ok(());
        }
        if !force
            && self
                .attempt
                .lock()
                .map_err(|_| "模型资料暂不可用")?
                .as_ref()
                .is_some_and(|(time, _)| time.elapsed() < Duration::from_secs(600))
        {
            return Ok(());
        }
        let downloaded = tokio::time::timeout(Duration::from_secs(30), async {
            let client = network::build_download_client(settings)?;
            let mut response = client
                .get("https://models.dev/api.json")
                .send()
                .await
                .map_err(|_| "模型资料网络请求失败")?;
            if !response.status().is_success() {
                return Err(format!(
                    "模型资料服务返回 HTTP {}",
                    response.status().as_u16()
                ));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| "模型资料下载中断")?
            {
                if bytes.len().saturating_add(chunk.len()) > MAX_BYTES {
                    return Err("模型资料超过读取容量".to_owned());
                }
                bytes.extend_from_slice(&chunk);
            }
            let models = tokio::task::spawn_blocking(move || {
                let root: Value = serde_json::from_slice(&bytes).map_err(|_| "模型资料格式无效")?;
                decode(&root)
            })
            .await
            .map_err(|_| "读取模型资料失败")??;
            Ok::<_, String>(Catalog {
                updated_at: chrono::Utc::now().timestamp(),
                models,
            })
        })
        .await
        .unwrap_or_else(|_| Err("模型资料请求超时".to_owned()));
        let result = match downloaded {
            Ok(catalog) => {
                let path = self.path.clone();
                // Keep the refresh gate until the bounded catalog is persisted.
                // A timed-out download can never commit after a newer refresh.
                let (catalog, saved) = tokio::task::spawn_blocking(move || {
                    let saved = cache::write(&path, &catalog);
                    (catalog, saved)
                })
                .await
                .map_err(|_| "保存模型资料失败")?;
                *self.catalog.lock().map_err(|_| "模型资料暂不可用")? = catalog;
                saved
            }
            Err(error) => Err(error),
        };
        *self.attempt.lock().map_err(|_| "模型资料暂不可用")? =
            Some((Instant::now(), result.as_ref().err().cloned()));
        result
    }
}

fn estimate(tokens: &SessionTokenCounts, prices: &ModelPrice) -> Option<u64> {
    let read = tokens.cache_read_tokens.unwrap_or_default();
    let write = tokens.cache_write_tokens.unwrap_or_default();
    let input = tokens.input_tokens?.checked_sub(read.checked_add(write)?)?;
    let cost = |tokens: u64, price: Option<f64>| {
        if tokens == 0 {
            Some(0.0)
        } else {
            price.map(|price| tokens as f64 * price)
        }
    };
    let micros = cost(input, prices.input)?
        + cost(tokens.output_tokens?, prices.output)?
        + cost(read, prices.cache_read)?
        + cost(write, prices.cache_write)?;
    (micros.is_finite() && micros >= 0.0 && micros < u64::MAX as f64)
        .then_some(micros.round() as u64)
}

fn decode(root: &Value) -> Result<BTreeMap<String, ModelMetadata>, String> {
    let providers = root.as_object().ok_or("模型资料缺少供应商目录")?;
    let mut candidates = BTreeMap::<String, Vec<ModelMetadata>>::new();
    for (provider, data) in providers {
        let Some(models) = data.get("models").and_then(Value::as_object) else {
            continue;
        };
        for (key, value) in models {
            let id = value.get("id").and_then(Value::as_str).unwrap_or(key);
            if id.is_empty() || id.len() > 512 {
                continue;
            }
            let modalities = |field: &str| {
                value
                    .pointer(field)
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(Value::as_str)
                            .take(12)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let price = |key: &str| {
                value
                    .get("cost")
                    .and_then(|cost| cost.get(key))
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite() && *value >= 0.0)
            };
            candidates
                .entry(id.to_owned())
                .or_default()
                .push(ModelMetadata {
                    id: id.to_owned(),
                    name: value
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(id)
                        .chars()
                        .take(256)
                        .collect(),
                    provider: provider.clone(),
                    context_window: value.pointer("/limit/context").and_then(Value::as_u64),
                    output_limit: value.pointer("/limit/output").and_then(Value::as_u64),
                    reasoning: value.get("reasoning").and_then(Value::as_bool),
                    tool_call: value.get("tool_call").and_then(Value::as_bool),
                    input_modalities: modalities("/modalities/input"),
                    output_modalities: modalities("/modalities/output"),
                    price: ModelPrice {
                        input: price("input"),
                        output: price("output"),
                        cache_read: price("cache_read"),
                        cache_write: price("cache_write"),
                    },
                });
        }
    }
    let mut result = BTreeMap::new();
    for (id, values) in candidates {
        // Resolve exact model IDs. Ambiguous reseller IDs remain provider-qualified.
        for value in &values {
            let qualified = format!("{}/{}", value.provider, value.id);
            result
                .entry(qualified.clone())
                .or_insert_with(|| ModelMetadata {
                    id: qualified,
                    ..value.clone()
                });
        }
        let native = values
            .iter()
            .filter(|value| {
                matches!(
                    value.provider.as_str(),
                    "openai" | "anthropic" | "google" | "xai" | "deepseek" | "mistral" | "cohere"
                )
            })
            .collect::<Vec<_>>();
        // Never choose a provider by map iteration order when IDs collide.
        let selected = if native.len() == 1 {
            native.first().copied()
        } else if values.len() == 1 {
            values.first()
        } else {
            None
        };
        if let Some(value) = selected {
            result.insert(id, value.clone());
        }
    }
    if result.is_empty() {
        return Err("模型资料目录为空".to_owned());
    }
    Ok(result)
}
