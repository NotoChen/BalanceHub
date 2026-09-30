//! Persistent version/configuration summaries. Resources belong to the catalog.
mod fingerprint;

use super::{cache, configuration::ConfigurationService};
use crate::models::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const CACHE_SCHEMA_VERSION: u32 = 5;

#[derive(Serialize, Deserialize)]
struct CachedOverview {
    schema_version: u32,
    signature: String,
    paths: BTreeSet<PathBuf>,
    snapshot: AgentOverviewSnapshot,
}

pub(crate) struct OverviewService {
    root: PathBuf,
    gates: Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

pub(crate) struct OverviewRefresh<'a> {
    pub kind: AgentCliKind,
    pub workspace: Option<&'a Path>,
    pub settings: &'a AppSettings,
    pub configuration: &'a ConfigurationService,
    pub force: bool,
}

impl OverviewService {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            gates: Mutex::default(),
        }
    }

    fn key(kind: AgentCliKind, workspace: Option<&Path>) -> String {
        let scope = format!(
            "{}\0{}\0{}",
            kind.key(),
            crate::services::cli_paths::user_home()
                .unwrap_or_default()
                .display(),
            workspace.unwrap_or_else(|| Path::new("")).display()
        );
        cache::key(scope.as_bytes())
    }

    fn load(&self, kind: AgentCliKind, workspace: Option<&Path>) -> Option<CachedOverview> {
        cache::read::<CachedOverview>(
            &self
                .root
                .join(format!("{}.json", Self::key(kind, workspace))),
        )
        .filter(|cached| {
            cached.schema_version == CACHE_SCHEMA_VERSION && cached.snapshot.agent_kind == kind
        })
    }

    pub(crate) fn cached(&self, workspace: Option<&Path>) -> Vec<AgentOverviewSnapshot> {
        super::definitions()
            .iter()
            .filter_map(|definition| {
                self.load(definition.kind, workspace)
                    .map(|cached| cached.snapshot)
            })
            .collect()
    }

    pub(crate) fn gate(
        &self,
        kind: AgentCliKind,
        workspace: Option<&Path>,
    ) -> Arc<tokio::sync::Mutex<()>> {
        self.gates
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entry(Self::key(kind, workspace))
            .or_default()
            .clone()
    }

    pub(crate) fn refresh(
        &self,
        request: OverviewRefresh<'_>,
    ) -> Result<AgentOverviewSnapshot, String> {
        let OverviewRefresh {
            kind,
            workspace,
            settings,
            configuration,
            force,
        } = request;
        let cached = self.load(kind, workspace);
        if !force {
            if let Some(cached) = cached.as_ref() {
                if !cached.signature.is_empty()
                    && fingerprint::signature(kind, settings, &cached.paths) == cached.signature
                {
                    return Ok(cached.snapshot.clone());
                }
            }
        }
        let actor = format!("overview:{}", Self::key(kind, workspace));
        let workspace_text = workspace.map(|path| path.to_string_lossy().into_owned());
        let known_paths = cached
            .as_ref()
            .map(|entry| entry.paths.clone())
            .unwrap_or_default();
        let before = fingerprint::signature(kind, settings, &known_paths);
        let mut paths = BTreeSet::new();
        let configuration_result = configuration.list_sources(
            &actor,
            AgentConfigurationListRequest {
                agent_kind: kind,
                workspace: workspace_text,
            },
            Some(settings),
        );
        let configuration_error = configuration_result
            .as_ref()
            .err()
            .map(|_| "读取配置来源失败，请刷新重试".to_owned())
            .unwrap_or_default();
        let configuration = configuration_result.ok().or_else(|| {
            cached
                .as_ref()
                .and_then(|entry| entry.snapshot.configuration.clone())
        });
        if let Some(configuration) = &configuration {
            for source in &configuration.sources {
                cache::include_path(&mut paths, Path::new(&source.path), None);
            }
        }
        let probe = super::probe_result(
            super::definition(kind),
            super::discovery::find_cli(
                settings.agent_cli_path(kind),
                super::definition(kind),
                false,
            ),
        );
        let snapshot = AgentOverviewSnapshot {
            agent_kind: kind,
            probe,
            configuration,
            configuration_error,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        let signature = if snapshot.configuration_error.is_empty()
            && before == fingerprint::signature(kind, settings, &known_paths)
        {
            fingerprint::signature(kind, settings, &paths)
        } else {
            String::new()
        };
        let cached = CachedOverview {
            schema_version: CACHE_SCHEMA_VERSION,
            signature,
            paths,
            snapshot: snapshot.clone(),
        };
        let _ = cache::write(
            &self
                .root
                .join(format!("{}.json", Self::key(kind, workspace))),
            &cached,
        );
        Ok(snapshot)
    }
}
