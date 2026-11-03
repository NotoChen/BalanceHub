use super::{definition::DefinitionPayload, CatalogService};
use crate::{
    models::*,
    services::agent_cli::maintenance::{self, files::preview_text, MaintenanceService},
};
use std::{collections::BTreeSet, sync::Arc};

impl CatalogService {
    pub(crate) fn definition_versions(&self, id: &str) -> Result<Vec<AgentSharedVersion>, String> {
        self.repository.read_entry(id, |entry| {
            Ok(entry
                .versions
                .iter()
                .rev()
                .map(|version| AgentSharedVersion {
                    version: version.version,
                    current: entry
                        .current()
                        .is_some_and(|current| current.version == version.version),
                    bytes: version.payload.comparison_bytes(),
                    file_count: match &version.payload {
                        DefinitionPayload::Skill(files) => files.len(),
                        _ => 1,
                    },
                })
                .collect())
        })
    }
    pub(crate) fn plan_definition_version(
        self: &Arc<Self>,
        actor: &str,
        service: &MaintenanceService,
        id: &str,
        version: u64,
    ) -> Result<AgentMaintenancePlan, String> {
        let (name, current, desired) = self.repository.read_entry(id, |entry| {
            Ok((
                entry.name.clone(),
                entry.current().cloned().ok_or("共享定义不存在")?,
                entry
                    .versions
                    .iter()
                    .find(|value| value.version == version)
                    .cloned()
                    .ok_or("此版本已不存在")?,
            ))
        })?;
        let changes = changes(&current.payload, &desired.payload);
        if changes.is_empty() {
            return Err("此版本与当前内容相同".to_owned());
        }
        let asset_id = id.to_owned();
        let catalog = Arc::clone(self);
        let mut display = maintenance::preview(
            AgentMaintenanceKind::SharedVersion,
            format!("恢复 {} 的共享版本 v{version}", name),
            changes,
            vec![maintenance::item(id, &name)],
        );
        display
            .notes
            .push("恢复内容会保存为新的共享版本；已配置的 Agent 可再选择更新。".to_owned());
        service.plan(
            actor,
            display,
            current.payload.comparison_bytes() + desired.payload.comparison_bytes(),
            move |context| {
                context.check()?;
                catalog.repository.transact(|library| {
                    let entry = library.entries.get_mut(&asset_id).ok_or("共享资产不存在")?;
                    if entry.current().map(|value| value.version) != Some(current.version) {
                        return Err("共享定义已变化，请重新预览".to_owned());
                    }
                    entry.set_definition(&name, desired.payload)?;
                    Ok(())
                })?;
                context.changed();
                context.item(
                    &asset_id,
                    AgentMaintenanceItemState::Completed,
                    Some("已保存新的共享版本".to_owned()),
                );
                Ok(())
            },
        )
    }
}

pub(super) fn changes(
    before: &DefinitionPayload,
    after: &DefinitionPayload,
) -> Vec<AgentAssetPlanChange> {
    if let (DefinitionPayload::Skill(before), DefinitionPayload::Skill(after)) = (before, after) {
        before
            .keys()
            .chain(after.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .flat_map(|path| {
                let old = before.get(path);
                let new = after.get(path);
                let mut changes = Vec::new();
                if old.zip(new).is_none_or(|(old, new)| old.bytes != new.bytes) {
                    changes.push(AgentAssetPlanChange {
                        label: path.clone(),
                        path: Some(path.clone()),
                        before: old.map(|file| preview_text(&file.bytes)),
                        after: new.map(|file| preview_text(&file.bytes)),
                    });
                }
                if old
                    .zip(new)
                    .is_some_and(|(old, new)| old.executable != new.executable)
                {
                    let mode = |file: &super::definition::PackageFile| {
                        if file.executable {
                            "mode 100755\n"
                        } else {
                            "mode 100644\n"
                        }
                        .to_owned()
                    };
                    changes.push(AgentAssetPlanChange {
                        label: format!("{path} · 执行权限"),
                        path: Some(path.clone()),
                        before: old.map(mode),
                        after: new.map(mode),
                    });
                }
                changes
            })
            .collect()
    } else if before.fingerprint() == after.fingerprint() {
        Vec::new()
    } else {
        vec![AgentAssetPlanChange {
            label: "共享定义".to_owned(),
            path: None,
            before: serde_json::to_string_pretty(before).ok(),
            after: serde_json::to_string_pretty(after).ok(),
        }]
    }
}
