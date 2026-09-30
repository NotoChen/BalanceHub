use crate::{
    limits,
    models::{AgentCliKind, AppData, TemporaryCliPreference, Workspace},
    services::workspaces::normalize_directory,
};
use std::path::Path;

use super::{MutationDecision, ProviderService};

impl ProviderService<'_> {
    pub fn record_temporary_cli_launch(
        &self,
        provider_id: &str,
        cli_kind: AgentCliKind,
        cli_path: &str,
        path: &Path,
        api_key_local_id: &str,
        model: &str,
    ) -> Result<(Vec<Workspace>, TemporaryCliPreference), String> {
        let normalized = normalize_directory(path)?.to_string_lossy().to_string();
        self.mutate(|data| {
            record_workspace_usage(data, cli_kind, cli_path, &normalized);
            let preference = TemporaryCliPreference {
                provider_id: provider_id.to_string(),
                cli_kind,
                api_key_local_id: api_key_local_id.trim().to_string(),
                model: model.trim().to_string(),
                workspace_path: normalized.clone(),
            };
            data.temporary_cli_preferences
                .retain(|item| item.provider_id != provider_id);
            data.temporary_cli_preferences.push(preference.clone());
            data.temporary_cli_preferences
                .sort_by(|left, right| left.provider_id.cmp(&right.provider_id));
            (data.workspaces.clone(), preference)
        })
    }

    pub fn record_native_cli_launch(
        &self,
        cli_kind: AgentCliKind,
        cli_path: &str,
        path: &Path,
    ) -> Result<Vec<Workspace>, String> {
        let normalized = normalize_directory(path)?.to_string_lossy().to_string();
        self.mutate(|data| {
            record_workspace_usage(data, cli_kind, cli_path, &normalized);
            data.workspaces.clone()
        })
    }

    pub fn forget_workspace(&self, path: String) -> Result<Vec<Workspace>, String> {
        self.mutate_decided(|data| {
            let workspace_count = data.workspaces.len();
            data.workspaces.retain(|workspace| workspace.path != path);
            let mut changed = workspace_count != data.workspaces.len();
            for preference in &mut data.temporary_cli_preferences {
                if preference.workspace_path == path {
                    preference.workspace_path.clear();
                    changed = true;
                }
            }
            if changed {
                sort_workspaces(&mut data.workspaces);
            }
            let workspaces = data.workspaces.clone();
            Ok(if changed {
                MutationDecision::changed(workspaces)
            } else {
                MutationDecision::unchanged(workspaces)
            })
        })
    }
}

fn record_workspace_usage(
    data: &mut AppData,
    cli_kind: AgentCliKind,
    cli_path: &str,
    normalized: &str,
) {
    data.settings
        .set_agent_cli_path(cli_kind, cli_path.to_string());
    if let Some(workspace) = data
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.path == normalized)
    {
        workspace.use_count = workspace.use_count.saturating_add(1);
    } else {
        data.workspaces.push(Workspace {
            path: normalized.to_string(),
            use_count: 1,
        });
    }
    sort_workspaces(&mut data.workspaces);
    data.workspaces.truncate(limits::MAX_WORKSPACES);
}

fn sort_workspaces(workspaces: &mut [Workspace]) {
    workspaces.sort_by(|left, right| {
        right
            .use_count
            .cmp(&left.use_count)
            .then_with(|| left.path.cmp(&right.path))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_workspace_usage_shares_history_without_provider_preferences() {
        let mut data = AppData::default();
        data.workspaces.push(Workspace {
            path: "/shared-project".to_string(),
            use_count: 2,
        });
        record_workspace_usage(
            &mut data,
            AgentCliKind::Codex,
            "/bin/codex",
            "/shared-project",
        );
        assert_eq!(data.workspaces.len(), 1);
        assert_eq!(data.workspaces[0].use_count, 3);
        assert!(data.temporary_cli_preferences.is_empty());
    }

    #[test]
    fn workspace_sort_prefers_higher_use_count() {
        let mut workspaces = vec![
            Workspace {
                path: "/frequent".to_string(),
                use_count: 9,
            },
            Workspace {
                path: "/occasional".to_string(),
                use_count: 1,
            },
        ];

        sort_workspaces(&mut workspaces);
        assert_eq!(workspaces[0].path, "/frequent");
    }
}
