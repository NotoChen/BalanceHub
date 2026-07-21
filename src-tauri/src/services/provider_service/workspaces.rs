use crate::{
    models::{LivenessCliKind, TemporaryCliPreference, Workspace},
    services::workspaces::normalize_directory,
};
use std::path::Path;

use super::ProviderService;

const MAX_WORKSPACES: usize = 30;

impl ProviderService<'_> {
    pub fn record_temporary_cli_launch(
        &self,
        provider_id: &str,
        cli_kind: LivenessCliKind,
        cli_path: &str,
        path: &Path,
        api_key_token_id: &str,
        model: &str,
    ) -> Result<(Vec<Workspace>, TemporaryCliPreference), String> {
        let normalized = normalize_directory(path)?.to_string_lossy().to_string();
        self.mutate(|data| {
            match cli_kind {
                LivenessCliKind::Codex => data.settings.codex_cli_path = cli_path.to_string(),
                LivenessCliKind::ClaudeCode => data.settings.claude_cli_path = cli_path.to_string(),
            }
            if let Some(workspace) = data
                .workspaces
                .iter_mut()
                .find(|workspace| workspace.path == normalized)
            {
                workspace.use_count = workspace.use_count.saturating_add(1);
            } else {
                data.workspaces.push(Workspace {
                    path: normalized.clone(),
                    use_count: 1,
                });
            }
            sort_workspaces(&mut data.workspaces);
            data.workspaces.truncate(MAX_WORKSPACES);
            let preference = TemporaryCliPreference {
                provider_id: provider_id.to_string(),
                cli_kind,
                api_key_token_id: api_key_token_id.trim().to_string(),
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

    pub fn forget_workspace(&self, path: String) -> Result<Vec<Workspace>, String> {
        self.mutate(|data| {
            data.workspaces.retain(|workspace| workspace.path != path);
            for preference in &mut data.temporary_cli_preferences {
                if preference.workspace_path == path {
                    preference.workspace_path.clear();
                }
            }
            sort_workspaces(&mut data.workspaces);
            data.workspaces.clone()
        })
    }
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
