use super::{
    hash,
    state::{self, ScopeState},
    AgentSessionActor,
};
use crate::{
    models::{AgentCliKind, AgentSessionScope, AgentSessionSource, AgentSessionWorkspace},
    services::{
        agent_cli::{
            self,
            contracts::{EnvironmentPatch, SessionHistoryAdapter},
        },
        cli_paths,
    },
    state::AppState,
};
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    time::Instant,
};
use tauri::Manager;

#[cfg(test)]
mod admission_fixture;
#[cfg(test)]
pub(crate) use admission_fixture::ResumeAdmissionFixture;

#[derive(Clone)]
pub(crate) struct ResolvedSessionTarget {
    pub cli_kind: AgentCliKind,
    pub workdir: PathBuf,
    pub native_session_id: String,
    pub session_ref: String,
    pub source_identity: String,
    pub config_root: PathBuf,
    pub can_resume: bool,
    pub resume_reason: Option<String>,
    pub launch_environment: EnvironmentPatch,
}

pub(super) fn history_adapter(
    kind: AgentCliKind,
) -> Result<&'static SessionHistoryAdapter, String> {
    Ok(agent_cli::definition(kind)
        .sessions()
        .ok_or_else(|| "Agent 不支持历史会话".to_string())?
        .history())
}

// Logical identity survives a temporarily missing mount. Canonical aliases are
// remembered while reachable; no case-folding is assumed on a case-sensitive volume.
pub(super) fn path_identity(path: &Path) -> String {
    let logical = lexical_path(path).to_string_lossy().to_string();
    let canonical = path
        .canonicalize()
        .ok()
        .map(|path| path.to_string_lossy().to_string());
    let mut registry = state::registry();
    // Reachable paths are resolved again: a retargeted symlink is a new scope.
    // Only a missing/unmounted logical path reuses its last proven identity.
    if let Some(canonical) = canonical {
        registry.path_identities.insert(logical, canonical.clone());
        registry
            .path_identities
            .insert(canonical.clone(), canonical.clone());
        canonical
    } else {
        registry
            .path_identities
            .entry(logical.clone())
            .or_insert(logical)
            .clone()
    }
}

fn lexical_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

fn workspace_scope(
    home: &Path,
    recorded: &[PathBuf],
    explicit_workdir: Option<&str>,
) -> Result<Vec<AgentSessionWorkspace>, String> {
    let mut paths = vec![home.to_path_buf()];
    paths.extend_from_slice(recorded);
    if let Some(explicit) = explicit_workdir {
        let path = PathBuf::from(explicit.trim());
        if !path.is_absolute() || explicit.chars().any(char::is_control) {
            return Err("显式工作目录必须是有效绝对路径".into());
        }
        paths = vec![path];
    }
    let home_identity = path_identity(home);
    let mut workspaces = Vec::<AgentSessionWorkspace>::new();
    for path in paths {
        if !path.is_absolute() {
            continue;
        }
        let identity = path_identity(&path);
        let id = hash(&["workspace", &identity]);
        if workspaces.iter().any(|workspace| workspace.id == id) {
            continue;
        }
        workspaces.push(AgentSessionWorkspace {
            id,
            path: identity.clone(),
            exists: Path::new(&identity).is_dir(),
            is_home: identity == home_identity,
        });
    }
    workspaces.sort_by(|left, right| {
        right
            .is_home
            .cmp(&left.is_home)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(workspaces)
}

pub(super) fn build_scope(
    actor: AgentSessionActor<'_>,
    explicit_workdir: Option<&str>,
) -> Result<ScopeState, String> {
    let home = cli_paths::user_home().ok_or_else(|| "无法定位原生用户主目录".to_string())?;
    let recorded = actor
        .app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .workspaces
        .iter()
        .map(|workspace| PathBuf::from(&workspace.path))
        .collect::<Vec<_>>();
    let workspaces = workspace_scope(&home, &recorded, explicit_workdir)?;
    let mut public_sources = Vec::new();
    let mut sources = HashMap::new();
    for definition in agent_cli::definitions() {
        let Ok(adapter) = history_adapter(definition.kind) else {
            continue;
        };
        let source = (adapter.source)()?;
        let identity = source
            .config_root
            .canonicalize()
            .unwrap_or_else(|_| lexical_path(&source.config_root))
            .to_string_lossy()
            .to_string();
        let id = hash(&["native", definition.kind.key(), &identity]);
        public_sources.push(AgentSessionSource {
            id: id.clone(),
            agent_kind: definition.kind,
            config_root: source.config_root.to_string_lossy().to_string(),
            available: source.config_root.is_dir(),
        });
        sources.insert(id, source);
    }
    let revision = hash(&[
        "scope-v1",
        &serde_json::to_string(&workspaces).map_err(|e| e.to_string())?,
        &serde_json::to_string(&public_sources).map_err(|e| e.to_string())?,
    ]);
    Ok(ScopeState {
        public: AgentSessionScope {
            revision,
            workspaces,
            sources: public_sources,
        },
        explicit_workdir: explicit_workdir.map(str::to_string),
        sources,
        created_at: Instant::now(),
    })
}

pub(super) fn session_identity(
    source: &AgentSessionSource,
    workspace: &AgentSessionWorkspace,
    record: &crate::services::agent_cli::contracts::SessionHistoryRecord,
) -> String {
    hash(&[
        "native",
        &source.id,
        &workspace.id,
        &record.record_key,
        &record.summary.id,
    ])
}

pub(crate) fn get_scope(
    actor: AgentSessionActor<'_>,
    explicit_workdir: Option<&str>,
) -> Result<AgentSessionScope, String> {
    let scope = build_scope(actor, explicit_workdir)?;
    let public = scope.public.clone();
    let mut registry = state::registry();
    registry.prune();
    registry
        .scopes
        .insert((actor.key(), public.revision.clone()), scope);
    Ok(public)
}

pub(super) fn validated_scope(
    actor: AgentSessionActor<'_>,
    revision: &str,
) -> Result<ScopeState, String> {
    validated_scope_with(&actor.key(), revision, |explicit_workdir| {
        build_scope(actor, explicit_workdir)
    })
}

fn validated_scope_with(
    actor_key: &str,
    revision: &str,
    build_current: impl FnOnce(Option<&str>) -> Result<ScopeState, String>,
) -> Result<ScopeState, String> {
    let saved = state::registry()
        .scopes
        .get(&(actor_key.into(), revision.into()))
        .cloned()
        .ok_or_else(|| "会话范围已失效，请刷新目录列表".to_string())?;
    if saved.created_at.elapsed().as_secs() >= 3600 {
        return Err("会话范围已过期，请刷新".into());
    }
    let current = build_current(saved.explicit_workdir.as_deref())?;
    if current.public.revision != revision {
        return Err("目录集合或原生会话来源已变化，请刷新".into());
    }
    Ok(current)
}

pub(crate) fn resolve_session_ref(
    actor: AgentSessionActor<'_>,
    scope_revision: &str,
    session_ref: &str,
) -> Result<ResolvedSessionTarget, String> {
    resolve_session_ref_with(
        &actor.key(),
        scope_revision,
        session_ref,
        |explicit_workdir| build_scope(actor, explicit_workdir),
    )
}

fn resolve_session_ref_with(
    actor_key: &str,
    scope_revision: &str,
    session_ref: &str,
    build_current: impl FnOnce(Option<&str>) -> Result<ScopeState, String>,
) -> Result<ResolvedSessionTarget, String> {
    let scope = validated_scope_with(actor_key, scope_revision, build_current)?;
    let entry = state::registry()
        .references
        .get(&(actor_key.into(), scope_revision.into(), session_ref.into()))
        .cloned()
        .ok_or_else(|| "会话引用已失效，请重新查询".to_string())?;
    let source = scope
        .sources
        .get(&entry.row.source_id)
        .ok_or_else(|| "原生会话来源已变化".to_string())?;
    let workspace = scope
        .public
        .workspaces
        .iter()
        .find(|workspace| workspace.id == entry.row.workspace_id)
        .ok_or_else(|| "工作目录已移出当前范围".to_string())?;
    if let Some(reason) = entry.record.content_unavailable_reason {
        return Err(reason);
    }
    let locator = entry
        .record
        .locator
        .canonicalize()
        .map_err(|_| "原生会话记录已不可用".to_string())?;
    let root = source
        .config_root
        .canonicalize()
        .map_err(|_| "原生会话数据根已不可用".to_string())?;
    if !locator.starts_with(&root) {
        return Err("原生会话记录已离开选定数据源".into());
    }
    let mut resume_reason = entry
        .row
        .resume_reason
        .clone()
        .or_else(|| source.resume_reason.clone());
    if !workspace.exists {
        resume_reason = Some("工作目录不存在或未挂载；仍可查看历史".into());
    }
    Ok(ResolvedSessionTarget {
        cli_kind: entry.record.summary.cli_kind,
        workdir: PathBuf::from(&workspace.path),
        native_session_id: entry.record.summary.id,
        session_ref: session_ref.into(),
        source_identity: hash(&[
            "native",
            &entry.row.source_id,
            &entry.row.workspace_id,
            &entry.record.record_key,
        ]),
        config_root: source.config_root.clone(),
        can_resume: entry.row.session.can_resume && resume_reason.is_none(),
        resume_reason,
        launch_environment: source.launch_environment.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "balancehub-session-scope-{name}-{}",
            std::process::id()
        ))
    }
    #[test]
    fn home_is_exact_and_explicit_workdir_can_be_unrecorded() {
        let root = root("exact");
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let child = home.join("unrecorded-child");
        let recorded = root.join("missing-mount");
        std::fs::create_dir_all(&child).unwrap();
        let scope = workspace_scope(&home, std::slice::from_ref(&recorded), None).unwrap();
        assert_eq!(scope.len(), 2);
        assert!(scope.iter().any(|item| item.is_home));
        assert!(!scope
            .iter()
            .any(|item| item.path == child.to_string_lossy()));
        let explicit = workspace_scope(&home, &[recorded], child.to_str()).unwrap();
        assert_eq!(explicit.len(), 1);
        assert_eq!(
            explicit[0].path,
            child.canonicalize().unwrap().to_string_lossy()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn aliases_order_and_unmounted_paths_keep_identity_but_retargets_do_not() {
        use std::os::unix::fs::symlink;
        let root = root("aliases");
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let first = root.join("first");
        let second = root.join("second");
        let alias = root.join("alias");
        for path in [&home, &first, &second] {
            std::fs::create_dir_all(path).unwrap();
        }
        symlink(&first, &alias).unwrap();
        let initial = workspace_scope(&home, &[first.clone(), alias.clone()], None).unwrap();
        let reordered = workspace_scope(&home, &[alias.clone(), first.clone()], None).unwrap();
        assert_eq!(
            initial, reordered,
            "use-count reordering must not change scope or display path"
        );
        let identity = path_identity(&alias);
        std::fs::remove_file(&alias).unwrap();
        assert_eq!(
            path_identity(&alias),
            identity,
            "a missing alias retains its last proven identity"
        );
        symlink(&second, &alias).unwrap();
        assert_ne!(
            path_identity(&alias),
            identity,
            "a retargeted alias must invalidate the old scope"
        );
        assert_eq!(
            workspace_scope(&home, &[first, alias], None).unwrap().len(),
            3
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
