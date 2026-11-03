use crate::{models::*, services::agent_cli::environment};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime},
};

#[derive(Clone)]
pub(super) struct LaunchAsset {
    pub option: TemporaryCliAssetOption,
    pub native_id: String,
    pub enabled: bool,
    pub parent: Option<AgentAssetNativeRef>,
    pub paths: Vec<PathBuf>,
}

#[derive(Clone, Default)]
pub(super) struct LaunchAssets {
    pub items: Vec<LaunchAsset>,
    sources: Vec<AgentAssetSource>,
    incomplete: BTreeSet<AgentAssetCategory>,
}

impl LaunchAssets {
    pub(super) fn require_complete(&self, category: AgentAssetCategory) -> Result<(), String> {
        if self.incomplete.contains(&category) {
            return Err(format!(
                "{} 资产尚未完整读取，无法保证整类停用；请刷新资产，或逐项选择",
                category_label(category)
            ));
        }
        Ok(())
    }

    pub(super) fn listing(&self) -> TemporaryCliAssetListing {
        TemporaryCliAssetListing {
            assets: self
                .items
                .iter()
                .filter(|a| a.enabled)
                .map(|a| a.option.clone())
                .collect(),
            notice: if self.incomplete.is_empty() {
                String::new()
            } else {
                format!(
                    "{} 的部分来源暂不可读，当前仅展示已读取的资产",
                    self.incomplete
                        .iter()
                        .map(|category| category_label(*category))
                        .collect::<Vec<_>>()
                        .join("、")
                )
            },
        }
    }

    // Reuse the adapter's native source boundaries and trust decision. Only
    // configuration files needed for this override are decoded, never Skill text.
    pub(super) fn configs(&self, filename: &str) -> Result<Vec<Value>, String> {
        self.sources
            .iter()
            .filter(|source| {
                Path::new(&source.path)
                    .file_name()
                    .is_some_and(|name| name == filename)
            })
            .map(read_config)
            .collect()
    }

    pub(super) fn has_managed_mcp(&self) -> bool {
        self.sources.iter().any(|source| {
            source.scope == AgentAssetScope::Managed
                && Path::new(&source.path)
                    .file_name()
                    .is_some_and(|name| name == "managed-mcp.json")
        })
    }
}

type SourceRevision = Option<(Option<SystemTime>, u64)>;

#[derive(Clone)]
struct CachedAssets {
    assets: LaunchAssets,
    revisions: Vec<(PathBuf, SourceRevision)>,
    created: Instant,
}

static CACHE: OnceLock<Mutex<BTreeMap<String, CachedAssets>>> = OnceLock::new();

pub(super) fn load(
    settings: &AppSettings,
    kind: AgentCliKind,
    workdir: &Path,
    refresh: bool,
) -> Result<LaunchAssets, String> {
    let key = format!(
        "{}:{}:{}",
        kind.key(),
        settings.agent_cli_path(kind),
        workdir.display()
    );
    let cache = CACHE.get_or_init(Mutex::default);
    let cached = cache
        .lock()
        .map_err(|_| "资产缓存不可用")?
        .get(&key)
        .cloned();
    if let Some(cached) = cached.filter(|entry| {
        !refresh
            && entry.created.elapsed().as_secs() < 60
            && entry
                .revisions
                .iter()
                .all(|(path, revision)| metadata_revision(path) == *revision)
    }) {
        return Ok(cached.assets);
    }
    let inventory = environment::display_inventory(
        settings,
        Some(workdir),
        Some(&BTreeSet::from([kind])),
        None,
    )?
    .inventory;
    let mut assets: BTreeMap<String, LaunchAsset> = BTreeMap::new();
    for asset in &inventory.assets {
        if asset.agent_kind != kind || asset.category == AgentAssetCategory::StatusUi {
            continue;
        }
        let id = asset_id(kind, asset.category, &asset.native_id);
        let bundled_mcp = kind == AgentCliKind::Codex
            && asset.relationships.provided_by.is_some()
            && asset.category == AgentAssetCategory::Mcp;
        let managed = asset.scope == AgentAssetScope::Managed;
        let supported = match kind {
            AgentCliKind::Codex | AgentCliKind::ClaudeCode => matches!(
                asset.category,
                AgentAssetCategory::Skill | AgentAssetCategory::Mcp | AgentAssetCategory::Plugin
            ),
            AgentCliKind::Gemini => matches!(
                asset.category,
                AgentAssetCategory::Skill | AgentAssetCategory::Mcp | AgentAssetCategory::Extension
            ),
            AgentCliKind::Grok => false,
        };
        let note = if managed {
            "由组织管理，按组织策略执行"
        } else if bundled_mcp {
            "由所属插件提供，随插件控制"
        } else if asset.category == AgentAssetCategory::Hook {
            "通过上方 Hook 开关统一控制"
        } else if !supported {
            "当前 Agent 不支持单次启动时独立停用此资产"
        } else if kind == AgentCliKind::ClaudeCode && asset.category == AgentAssetCategory::Skill {
            "本次禁止调用此 Skill"
        } else {
            ""
        };
        let enabled = matches!(
            asset.effective_state,
            AgentAssetState::Enabled | AgentAssetState::Unknown
        ) && !matches!(
            asset.trust_state,
            AgentTrustState::Untrusted | AgentTrustState::Required
        );
        let entry = assets.entry(id.clone()).or_insert_with(|| LaunchAsset {
            option: TemporaryCliAssetOption {
                id,
                label: asset.label.clone(),
                category: asset.category,
                configurable: supported && !managed && !bundled_mcp,
                note: note.into(),
                current_label: if asset.effective_state == AgentAssetState::Enabled {
                    "原生已启用"
                } else {
                    "原生状态待确认"
                }
                .into(),
            },
            native_id: asset.native_id.clone(),
            enabled: false,
            parent: asset
                .relationships
                .provided_by
                .as_deref()
                .and_then(|id| {
                    inventory
                        .assets
                        .iter()
                        .find(|candidate| candidate.stable_id == id)
                })
                .map(|parent| AgentAssetNativeRef {
                    category: parent.category,
                    native_id: parent.native_id.clone(),
                    qualifier: None,
                }),
            paths: Vec::new(),
        });
        entry.enabled |= enabled;
        if asset.effective_state == AgentAssetState::Enabled {
            entry.option.current_label = "原生已启用".into();
        }
        if let Some(path) = &asset.path {
            let path = PathBuf::from(path);
            if !entry.paths.contains(&path) {
                entry.paths.push(path);
            }
        }
    }
    let mut incomplete = BTreeSet::new();
    let all_categories = [
        AgentAssetCategory::Skill,
        AgentAssetCategory::Mcp,
        AgentAssetCategory::Plugin,
        AgentAssetCategory::Extension,
    ];
    collect_incomplete(&inventory.diagnostics, &all_categories, &mut incomplete);
    for source in &inventory.sources {
        collect_incomplete(&source.diagnostics, &source.categories, &mut incomplete);
    }
    for asset in &inventory.assets {
        collect_incomplete(&asset.diagnostics, &[asset.category], &mut incomplete);
    }
    let revisions = inventory
        .sources
        .iter()
        .map(|source| {
            let path = PathBuf::from(&source.path);
            let revision = metadata_revision(&path);
            (path, revision)
        })
        .collect();
    let mut sources: Vec<_> = inventory
        .sources
        .into_iter()
        .filter(|source| {
            !source.revision.is_missing
                && source.source_kind == AgentAssetSourceKind::File
                && (!matches!(
                    source.scope,
                    AgentAssetScope::Workspace | AgentAssetScope::Local
                ) || inventory.contexts.iter().any(|context| {
                    context.id == source.context_id
                        && context.trust_context == AgentTrustState::Trusted
                }))
        })
        .collect();
    sources.sort_by_key(|source| source.precedence);
    let result = LaunchAssets {
        items: assets.into_values().collect(),
        sources,
        incomplete,
    };
    let mut cache = cache.lock().map_err(|_| "资产缓存不可用")?;
    if cache.len() >= 8 {
        cache.clear();
    }
    cache.insert(
        key,
        CachedAssets {
            assets: result.clone(),
            revisions,
            created: Instant::now(),
        },
    );
    Ok(result)
}

fn metadata_revision(path: &Path) -> SourceRevision {
    std::fs::metadata(path)
        .ok()
        .map(|metadata| (metadata.modified().ok(), metadata.len()))
}

fn collect_incomplete(
    diagnostics: &[AgentAssetDiagnostic],
    categories: &[AgentAssetCategory],
    result: &mut BTreeSet<AgentAssetCategory>,
) {
    for diagnostic in diagnostics {
        match diagnostic {
            AgentAssetDiagnostic::Truncated { .. }
            | AgentAssetDiagnostic::BudgetExceeded { .. }
            | AgentAssetDiagnostic::ReadFailed { .. }
            | AgentAssetDiagnostic::Malformed { .. }
            | AgentAssetDiagnostic::SymlinkRejected { .. }
            | AgentAssetDiagnostic::SourceOutsideAllowedRoot { .. }
            | AgentAssetDiagnostic::SourceTypeMismatch { .. }
            | AgentAssetDiagnostic::UnresolvedRelationship { .. } => result.extend(categories),
            AgentAssetDiagnostic::DiscoveryIncomplete {
                category, reason, ..
            } if !matches!(
                reason,
                AgentAssetDiscoveryIncompleteReason::RuntimeStateUnobserved
                    | AgentAssetDiscoveryIncompleteReason::NativeEquivalenceUnobserved
            ) =>
            {
                result.insert(*category);
            }
            _ => {}
        }
    }
}

fn read_config(source: &AgentAssetSource) -> Result<Value, String> {
    use environment::{
        config_document::{self, ConfigDocumentFormat},
        verified_path::inspect_verified_path,
    };
    let root = Path::new(&source.allowed_root);
    let guard = inspect_verified_path(
        &[root],
        root,
        Path::new(&source.path),
        AgentAssetSourceKind::File,
    )
    .map_err(|_| format!("{} 已变化或不可读，请刷新资产", source.label))?;
    let bytes = guard
        .read_file_bounded(8 * 1024 * 1024)
        .map_err(|_| format!("读取 {} 失败，请核对原生配置", source.label))?;
    let format = if source.path.ends_with(".toml") {
        ConfigDocumentFormat::Toml
    } else {
        ConfigDocumentFormat::Jsonc
    };
    config_document::parse(&bytes, format)
        .ok_or_else(|| format!("{} 格式无效，无法保留原有规则", source.label))
}

pub(super) fn requested(settings: &TemporaryCliRunSettings, asset: &LaunchAsset) -> bool {
    settings.disabled_assets.contains(&asset.option.id)
        || match asset.option.category {
            AgentAssetCategory::Skill => settings.disable_skills,
            AgentAssetCategory::Mcp => settings.disable_mcp,
            AgentAssetCategory::Plugin | AgentAssetCategory::Extension => settings.disable_plugins,
            AgentAssetCategory::Hook => settings.disable_hooks,
            AgentAssetCategory::StatusUi => false,
        }
}

pub(super) fn asset_id(
    kind: AgentCliKind,
    category: AgentAssetCategory,
    native_id: &str,
) -> String {
    let category = match category {
        AgentAssetCategory::Skill => "skill",
        AgentAssetCategory::Mcp => "mcp",
        AgentAssetCategory::Plugin => "plugin",
        AgentAssetCategory::Extension => "extension",
        AgentAssetCategory::Hook => "hook",
        AgentAssetCategory::StatusUi => "statusUi",
    };
    format!("{}:{category}:{native_id}", kind.key())
}

fn category_label(category: AgentAssetCategory) -> &'static str {
    match category {
        AgentAssetCategory::Skill => "Skill",
        AgentAssetCategory::Mcp => "MCP",
        AgentAssetCategory::Plugin => "插件",
        AgentAssetCategory::Extension => "扩展",
        AgentAssetCategory::Hook => "Hook",
        AgentAssetCategory::StatusUi => "状态栏",
    }
}
