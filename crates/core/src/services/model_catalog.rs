//! Optional descriptions from models.dev. This service never grants availability.
use crate::{models::*, network, util::cache};
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
pub struct ModelCatalogService {
    path: PathBuf,
    catalog: Mutex<Catalog>,
    gate: tokio::sync::Mutex<()>,
    attempt: Mutex<Option<(Instant, Option<String>)>>,
}
impl ModelCatalogService {
    pub fn estimate(&self, usage: SessionUsageSummary) -> Result<SessionUsageSummary, String> {
        self.estimate_with(usage, None, None)
    }

    /// 与 `estimate` 同一套价格计算，但允许按中转站卡片的模型高级配置覆盖：
    /// `catalog` 声明优先的 models.dev 供应商目录，`overrides[id].price`
    /// 按字段覆盖目录价格。站点价目优先于公开目录，手动覆盖仍优先于站点价。
    pub fn estimate_with(
        &self,
        mut usage: SessionUsageSummary,
        config: Option<&ProviderModelConfig>,
        site_pricing: Option<&ProviderPricingSnapshot>,
    ) -> Result<SessionUsageSummary, String> {
        let ids = usage
            .models
            .iter()
            .filter_map(|model| model.model.clone())
            .collect::<Vec<_>>();
        let prices = self
            .snapshot_for(&ids, config, site_pricing)?
            .models
            .into_iter()
            .map(|metadata| (metadata.id, metadata.price))
            .collect::<BTreeMap<_, _>>();
        usage.estimated_cost_microusd = None;
        usage.unpriced_records = 0;
        for model in &mut usage.models {
            model.estimated_cost_microusd = model
                .model
                .as_ref()
                .and_then(|id| prices.get(id))
                .and_then(|price| estimate(&model.tokens, price));
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
    pub fn new(path: PathBuf) -> Self {
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
    /// 同一份 models.dev 缓存按卡片的高级配置做覆盖：卡片声明的名称、上下文、
    /// 输出上限和价格优先于上游目录，缺失字段仍由目录补全。站点价目优先于
    /// 公开目录，手动覆盖再优先于站点价。
    pub fn snapshot_for(
        &self,
        ids: &[String],
        config: Option<&ProviderModelConfig>,
        site_pricing: Option<&ProviderPricingSnapshot>,
    ) -> Result<ModelMetadataSnapshot, String> {
        let catalog = self.catalog.lock().map_err(|_| "模型资料暂不可用")?;
        let models = ids
            .iter()
            .filter_map(|id| {
                let catalog_name = config
                    .map(|config| config.catalog.trim())
                    .filter(|name| !name.is_empty());
                let lookup = catalog_name
                    .and_then(|name| catalog.models.get(&format!("{name}/{id}")))
                    .or_else(|| catalog.models.get(id));
                let over = config.and_then(|config| config.overrides.get(id));
                // 站点价目按上游 wire name 返回；卡片把展示名映射到另一个
                // 上游模型时，两种名称都要查。
                let site = site_pricing.and_then(|pricing| {
                    if let Some(price) = pricing.model(id) {
                        return Some(price);
                    }
                    let wire = config.map_or(id.as_str(), |config| config.wire_name(id));
                    if wire == id.as_str() {
                        None
                    } else {
                        pricing.model(wire)
                    }
                });
                if lookup.is_none() && over.is_none() && site.is_none() {
                    return None;
                }
                let mut metadata = match lookup {
                    Some(base) => {
                        let mut metadata = base.clone();
                        metadata.id.clone_from(id);
                        metadata
                    }
                    None => ModelMetadata {
                        id: id.clone(),
                        name: id.clone(),
                        provider: String::new(),
                        context_window: None,
                        output_limit: None,
                        reasoning: None,
                        tool_call: None,
                        input_modalities: Vec::new(),
                        output_modalities: Vec::new(),
                        price: ModelPrice::default(),
                        per_request: None,
                        price_source: crate::models::ModelPriceSource::None,
                        price_group: String::new(),
                    },
                };
                if let Some(site) = site {
                    apply_site_price(&mut metadata, site);
                }
                Some(over.map_or(metadata.clone(), |over| {
                    apply_model_override(metadata, over)
                }))
            })
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
    pub async fn refresh(&self, settings: &AppSettings, force: bool) -> Result<(), String> {
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

fn apply_site_price(metadata: &mut ModelMetadata, site: &ProviderModelPrice) {
    if let Some(per_request) = site.per_request {
        // 按次计费的模型没有可换算的 token 单价；用站点单价替换目录参考价，
        // 避免把不适用的 token 估算继续展示成这个模型的费用。
        metadata.price = ModelPrice::default();
        metadata.per_request = Some(per_request);
        metadata.price_source = ModelPriceSource::Site;
        metadata.price_group.clone_from(&site.group);
        return;
    }
    if !site.has_token_price() {
        return;
    }
    metadata.price = site.token_price();
    metadata.per_request = None;
    metadata.price_source = ModelPriceSource::Site;
    metadata.price_group.clone_from(&site.group);
}

fn apply_model_override(
    mut metadata: ModelMetadata,
    over: &ProviderModelOverride,
) -> ModelMetadata {
    if !over.display_name.trim().is_empty() {
        metadata.name = over.display_name.trim().to_string();
    }
    metadata.context_window = over.context_window.or(metadata.context_window);
    metadata.output_limit = over.output_limit.or(metadata.output_limit);
    if over.sends_reasoning == Some(false) {
        metadata.reasoning = Some(false);
    } else if !over.reasoning_levels.is_empty() {
        metadata.reasoning = Some(true);
    }
    let overrides_price = [
        over.price.input,
        over.price.output,
        over.price.cache_read,
        over.price.cache_write,
    ]
    .into_iter()
    .any(|price| price.is_some());
    metadata.price = ModelPrice {
        input: over.price.input.or(metadata.price.input),
        output: over.price.output.or(metadata.price.output),
        cache_read: over.price.cache_read.or(metadata.price.cache_read),
        cache_write: over.price.cache_write.or(metadata.price.cache_write),
    };
    if overrides_price {
        metadata.price_source = ModelPriceSource::Manual;
        metadata.per_request = None;
        metadata.price_group.clear();
    }
    metadata
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
            let model_price = ModelPrice {
                input: price("input"),
                output: price("output"),
                cache_read: price("cache_read"),
                cache_write: price("cache_write"),
            };
            let has_price = [
                model_price.input,
                model_price.output,
                model_price.cache_read,
                model_price.cache_write,
            ]
            .into_iter()
            .any(|price| price.is_some());
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
                    price: model_price,
                    per_request: None,
                    price_source: if has_price {
                        ModelPriceSource::Catalog
                    } else {
                        ModelPriceSource::None
                    },
                    price_group: String::new(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(id: &str, price: ModelPrice) -> ModelMetadata {
        ModelMetadata {
            id: id.into(),
            name: id.into(),
            provider: String::new(),
            context_window: None,
            output_limit: None,
            reasoning: None,
            tool_call: None,
            input_modalities: Vec::new(),
            output_modalities: Vec::new(),
            price,
            per_request: None,
            price_source: ModelPriceSource::Catalog,
            price_group: String::new(),
        }
    }

    fn service() -> ModelCatalogService {
        let service =
            ModelCatalogService::new(std::env::temp_dir().join("balancehub-catalog-test"));
        let mut catalog = service.catalog.lock().unwrap();
        catalog.updated_at = chrono::Utc::now().timestamp();
        catalog.models.insert(
            "claude-sonnet-4-5".into(),
            metadata(
                "claude-sonnet-4-5",
                ModelPrice {
                    input: Some(3.0),
                    output: Some(15.0),
                    cache_read: Some(0.3),
                    cache_write: Some(3.75),
                },
            ),
        );
        catalog.models.insert(
            "claude-haiku-4-5".into(),
            metadata(
                "claude-haiku-4-5",
                ModelPrice {
                    input: Some(1.0),
                    output: Some(5.0),
                    ..ModelPrice::default()
                },
            ),
        );
        drop(catalog);
        service
    }

    fn usage() -> SessionUsageSummary {
        SessionUsageSummary {
            models: vec![SessionModelUsage {
                model: Some("claude-sonnet-4-5".into()),
                tokens: SessionTokenCounts {
                    input_tokens: Some(1_000_000),
                    output_tokens: Some(100_000),
                    cache_read_tokens: Some(200_000),
                    ..SessionTokenCounts::default()
                },
                records: 2,
                estimated_cost_microusd: None,
            }],
            records: 2,
            ..SessionUsageSummary::default()
        }
    }

    #[test]
    fn estimate_with_uses_card_overrides_by_field() {
        let service = service();
        let config = ProviderModelConfig {
            overrides: BTreeMap::from([(
                "claude-sonnet-4-5".to_string(),
                ProviderModelOverride {
                    price: ModelPrice {
                        output: Some(30.0),
                        ..ModelPrice::default()
                    },
                    ..ProviderModelOverride::default()
                },
            )]),
            ..ProviderModelConfig::default()
        };
        let priced = service.estimate_with(usage(), Some(&config), None).unwrap();
        // 输入 800k × $3 + 缓存读 200k × $0.3 + 输出 100k × 覆盖后的 $30
        assert_eq!(priced.estimated_cost_microusd, Some(5_460_000));
        assert_eq!(priced.unpriced_records, 0);
    }

    #[test]
    fn site_prices_override_catalog_prices_and_keep_their_group() {
        let service = service();
        let site = ProviderPricingSnapshot {
            models: BTreeMap::from([(
                "claude-sonnet-4-5".to_string(),
                ProviderModelPrice {
                    input: Some(1.0),
                    output: Some(5.0),
                    cache_read: Some(0.1),
                    cache_write: Some(1.25),
                    group: "vip".to_string(),
                    ..ProviderModelPrice::default()
                },
            )]),
            ..ProviderPricingSnapshot::default()
        };
        let snapshot = service
            .snapshot_for(&["claude-sonnet-4-5".to_string()], None, Some(&site))
            .unwrap();
        let model = &snapshot.models[0];
        assert_eq!(model.price.input, Some(1.0));
        assert_eq!(model.price_source, ModelPriceSource::Site);
        assert_eq!(model.price_group, "vip");

        let priced = service.estimate_with(usage(), None, Some(&site)).unwrap();
        // 输入 800k × $1 + 缓存读 200k × $0.1 + 输出 100k × $5
        assert_eq!(priced.estimated_cost_microusd, Some(1_320_000));
    }

    #[test]
    fn per_request_site_price_replaces_catalog_token_price() {
        let service = service();
        let site = ProviderPricingSnapshot {
            models: BTreeMap::from([(
                "claude-sonnet-4-5".to_string(),
                ProviderModelPrice {
                    per_request: Some(0.04),
                    group: "vip".to_string(),
                    ..ProviderModelPrice::default()
                },
            )]),
            ..ProviderPricingSnapshot::default()
        };
        let snapshot = service
            .snapshot_for(&["claude-sonnet-4-5".to_string()], None, Some(&site))
            .unwrap();
        let model = &snapshot.models[0];
        assert_eq!(model.per_request, Some(0.04));
        assert_eq!(model.price.input, None);
        assert_eq!(model.price_source, ModelPriceSource::Site);
        assert_eq!(model.price_group, "vip");
    }

    #[test]
    fn site_price_lookup_uses_the_configured_wire_name() {
        let service = service();
        let site = ProviderPricingSnapshot {
            models: BTreeMap::from([(
                "upstream-model-id".to_string(),
                ProviderModelPrice {
                    input: Some(2.0),
                    output: Some(6.0),
                    ..ProviderModelPrice::default()
                },
            )]),
            ..ProviderPricingSnapshot::default()
        };
        let config = ProviderModelConfig {
            overrides: BTreeMap::from([(
                "display-model".to_string(),
                ProviderModelOverride {
                    wire_name: "upstream-model-id".to_string(),
                    ..ProviderModelOverride::default()
                },
            )]),
            ..ProviderModelConfig::default()
        };
        let snapshot = service
            .snapshot_for(&["display-model".to_string()], Some(&config), Some(&site))
            .unwrap();
        assert_eq!(snapshot.models[0].price.input, Some(2.0));
        assert_eq!(snapshot.models[0].price_source, ModelPriceSource::Site);
    }

    #[test]
    fn manual_override_wins_over_site_price() {
        let service = service();
        let site = ProviderPricingSnapshot {
            models: BTreeMap::from([(
                "claude-sonnet-4-5".to_string(),
                ProviderModelPrice {
                    input: Some(1.0),
                    output: Some(5.0),
                    ..ProviderModelPrice::default()
                },
            )]),
            ..ProviderPricingSnapshot::default()
        };
        let config = ProviderModelConfig {
            overrides: BTreeMap::from([(
                "claude-sonnet-4-5".to_string(),
                ProviderModelOverride {
                    price: ModelPrice {
                        output: Some(30.0),
                        ..ModelPrice::default()
                    },
                    ..ProviderModelOverride::default()
                },
            )]),
            ..ProviderModelConfig::default()
        };
        let snapshot = service
            .snapshot_for(
                &["claude-sonnet-4-5".to_string()],
                Some(&config),
                Some(&site),
            )
            .unwrap();
        let model = &snapshot.models[0];
        assert_eq!(model.price.input, Some(1.0));
        assert_eq!(model.price.output, Some(30.0));
        assert_eq!(model.price_source, ModelPriceSource::Manual);
    }

    #[test]
    fn site_prices_can_describe_models_missing_from_the_catalog() {
        let service = service();
        let site = ProviderPricingSnapshot {
            models: BTreeMap::from([(
                "site-only-model".to_string(),
                ProviderModelPrice {
                    input: Some(0.5),
                    output: Some(1.5),
                    ..ProviderModelPrice::default()
                },
            )]),
            ..ProviderPricingSnapshot::default()
        };
        let snapshot = service
            .snapshot_for(&["site-only-model".to_string()], None, Some(&site))
            .unwrap();
        assert_eq!(snapshot.models.len(), 1);
        assert_eq!(snapshot.models[0].price.input, Some(0.5));
        assert_eq!(snapshot.models[0].price_source, ModelPriceSource::Site);
    }

    #[test]
    fn estimate_keeps_unknown_models_unpriced() {
        let service = service();
        let mut usage = usage();
        usage.models.push(SessionModelUsage {
            model: None,
            tokens: SessionTokenCounts {
                input_tokens: Some(10),
                output_tokens: Some(10),
                ..SessionTokenCounts::default()
            },
            records: 1,
            estimated_cost_microusd: None,
        });
        let priced = service.estimate(usage).unwrap();
        assert_eq!(priced.estimated_cost_microusd, Some(3_960_000));
        assert_eq!(priced.unpriced_records, 1);
    }
}
