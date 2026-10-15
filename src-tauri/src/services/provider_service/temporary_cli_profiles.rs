use super::{MutationDecision, ProviderService};
use crate::models::TemporaryCliProfile;

impl ProviderService<'_> {
    pub(crate) fn save_temporary_cli_profile(
        &self,
        mut profile: TemporaryCliProfile,
    ) -> Result<Vec<TemporaryCliProfile>, String> {
        profile.name = profile.name.trim().to_owned();
        if profile.name.is_empty() || profile.name.chars().count() > 60 {
            return Err("方案名称需为 1–60 个字符".into());
        }
        crate::services::temporary_cli::validate_run_settings(&profile.settings)?;
        self.mutate_decided(|data| {
            if profile.id.is_empty() {
                let base = format!("launch-{}", crate::util::unix_millis());
                profile.id = base.clone();
                let mut sequence = 0;
                while data
                    .temporary_cli_profiles
                    .iter()
                    .any(|p| p.id == profile.id)
                {
                    sequence += 1;
                    profile.id = format!("{base}-{sequence}");
                }
                data.temporary_cli_profiles.push(profile);
            } else {
                let current = data
                    .temporary_cli_profiles
                    .iter_mut()
                    .find(|p| p.id == profile.id)
                    .ok_or("启动方案已被删除，请另存为新方案")?;
                *current = profile;
            }
            Ok(MutationDecision::changed(
                data.temporary_cli_profiles.clone(),
            ))
        })
    }

    pub(crate) fn delete_temporary_cli_profile(
        &self,
        id: &str,
    ) -> Result<Vec<TemporaryCliProfile>, String> {
        self.mutate_decided(|data| {
            data.temporary_cli_profiles
                .retain(|profile| profile.id != id);
            Ok(MutationDecision::changed(
                data.temporary_cli_profiles.clone(),
            ))
        })
    }
}
