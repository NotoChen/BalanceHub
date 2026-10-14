mod discovery;
mod github;

use super::{definition::DefinitionPayload, CatalogService, PublishedCatalog, ReadControl};
use crate::{
    models::*,
    services::agent_cli::maintenance::{self, MaintenanceService},
};
pub(super) use github::normalize;
pub(super) use github::UpstreamCache;
use std::sync::Arc;

impl CatalogService {
    pub(crate) fn skill_origin(
        &self,
        id: &str,
        publication: Option<&PublishedCatalog>,
    ) -> Result<Option<AgentSkillOrigin>, String> {
        let stored = self.repository.read_entry(id, |entry| {
            if entry.category != AgentAssetCategory::Skill {
                return Err("此资源不是 Skill".to_owned());
            }
            Ok(entry.origin.clone())
        })?;
        if stored.is_some() {
            return Ok(stored);
        }
        let item = publication
            .and_then(|publication| publication.catalog.assets.iter().find(|item| item.id == id));
        Ok(item.and_then(discovery::discover))
    }
    pub(crate) fn save_skill_origin(
        &self,
        id: &str,
        version: u64,
        origin: Option<AgentSkillOrigin>,
    ) -> Result<Option<AgentSkillOrigin>, String> {
        let origin = origin.map(github::normalize).transpose()?;
        self.repository.transact(|library| {
            let entry = library.entries.get_mut(id).ok_or("请先保存到共享库")?;
            if entry.category != AgentAssetCategory::Skill
                || entry.current().map(|value| value.version) != Some(version)
            {
                return Err("Skill 版本已变化，请重新读取".to_owned());
            }
            entry.origin = origin.clone();
            Ok(origin)
        })
    }
    pub(crate) async fn check_skill_update(
        self: &Arc<Self>,
        id: &str,
        settings: &AppSettings,
        control: &ReadControl,
    ) -> Result<AgentSkillUpdateCheck, String> {
        let catalog = Arc::clone(self);
        let id = id.to_owned();
        let read_control = control.clone();
        let origin = tauri::async_runtime::spawn_blocking(move || {
            read_control.check()?;
            catalog.repository.read_entry(&id, |entry| {
                entry
                    .origin
                    .clone()
                    .ok_or_else(|| "请先确认此 Skill 的来源仓库".to_owned())
            })
        })
        .await
        .map_err(|_| "读取来源配置失败")??;
        control.check()?;
        let client = crate::network::build_download_client(settings)?;
        let head = self.upstream.head(&client, &origin).await?;
        control.check()?;
        let changed = origin.revision.as_deref() != Some(&head.revision);
        Ok(AgentSkillUpdateCheck {
            revision: head.revision,
            changed,
            checked_at: chrono::Local::now().to_rfc3339(),
            message: if origin.revision.is_none() {
                "来源尚未关联版本，可比较上游内容"
            } else if changed {
                "此 Skill 目录有新的上游版本"
            } else {
                "此 Skill 已关联最新的上游版本"
            }
            .to_owned(),
        })
    }
    pub(crate) async fn plan_skill_update(
        self: &Arc<Self>,
        actor: &str,
        maintenance: &Arc<MaintenanceService>,
        id: &str,
        settings: &AppSettings,
        control: &ReadControl,
    ) -> Result<AgentMaintenancePlan, String> {
        let catalog = Arc::clone(self);
        let asset_id = id.to_owned();
        let read_control = control.clone();
        let (name, current, origin) = tauri::async_runtime::spawn_blocking(move || {
            read_control.check()?;
            catalog.repository.read_entry(&asset_id, |entry| {
                Ok((
                    entry.name.clone(),
                    entry.current().cloned().ok_or("请先保存到共享库")?,
                    entry.origin.clone().ok_or("请先确认来源仓库")?,
                ))
            })
        })
        .await
        .map_err(|_| "读取共享 Skill 失败")??;
        control.check()?;
        let client = crate::network::build_download_client(settings)?;
        let head = self.upstream.head(&client, &origin).await?;
        let files = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            github::download(&client, &origin, &head),
        )
        .await
        .map_err(|_| "读取上游 Skill 超时")??;
        control.check()?;
        let catalog = Arc::clone(self);
        let id = id.to_owned();
        let actor = actor.to_owned();
        let maintenance = Arc::clone(maintenance);
        let control = control.clone();
        tauri::async_runtime::spawn_blocking(move || {
        control.check()?;
        let desired = DefinitionPayload::Skill(files);
        let changes = super::version_history::changes(&current.payload, &desired);
        let mut display = maintenance::preview(AgentMaintenanceKind::SkillUpdate, format!("更新 {name} 的共享版本"), changes, vec![maintenance::item(&id, &name)]);
        display.notes.push(format!("来源：{} / {} · {}", origin.repository, if origin.directory.is_empty() { "/" } else { &origin.directory }, &head.revision[..head.revision.len().min(12)]));
        display.notes.push("左侧为当前共享内容，包含你的本地修改；确认后保存上游内容为新版本。旧版本仍可恢复，已配置的 Agent 可再选择更新。".to_owned());
        control.check()?;
        maintenance.plan(&actor, display, current.payload.comparison_bytes() + desired.comparison_bytes(), move |context| {
            context.check()?;
            catalog.repository.transact(|library| {
                let entry = library.entries.get_mut(&id).ok_or("Skill 已不存在")?;
                if entry.current().map(|value| value.version) != Some(current.version) || entry.origin.as_ref() != Some(&origin) { return Err("共享内容或来源在预览后已变化，请重新比较".to_owned()); }
                entry.set_definition(&name, desired)?;
                entry.origin = Some(AgentSkillOrigin { revision: Some(head.revision), ..origin });
                Ok(())
            })?;
            context.changed();
            context.item(&id, AgentMaintenanceItemState::Completed, Some("共享版本已更新，可选择配置到 Agent".to_owned()));
            Ok(())
        })
        }).await.map_err(|_| "准备上游内容失败".to_owned())?
    }
}
