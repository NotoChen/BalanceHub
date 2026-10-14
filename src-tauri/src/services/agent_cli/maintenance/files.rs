use super::TaskContext;
use crate::{
    models::*,
    services::agent_cli::{
        config_support::{file_writer::StagedFile, native_history},
        environment::{mutation::GuardedFile, verified_path::removal},
    },
};

pub(crate) struct FileWrite {
    pub id: String,
    pub source: GuardedFile,
    pub after: Option<Vec<u8>>,
    pub executable: Option<bool>,
    pub mode: Option<u32>,
}

impl FileWrite {
    pub(crate) fn bytes(&self) -> usize {
        self.source.bytes().map_or(0, <[u8]>::len) + self.after.as_ref().map_or(0, Vec::len)
    }
    pub(crate) fn change(&self) -> AgentAssetPlanChange {
        AgentAssetPlanChange {
            label: self
                .source
                .path()
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            path: Some(
                crate::platform::paths::compatible_path(self.source.path())
                    .to_string_lossy()
                    .into_owned(),
            ),
            before: self.source.bytes().map(preview_text),
            after: self.after.as_deref().map(preview_text),
        }
    }
}

/// Only the selected file crosses IPC. Binary resources retain their byte-level
/// identity without lossy text conversion; text diffs include the complete file.
pub(crate) fn preview_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) if !text.contains('\0') => text.to_owned(),
        _ => format!(
            "二进制文件 · {} 字节\nSHA-256: {}",
            bytes.len(),
            native_history::hash(bytes)
        ),
    }
}

pub(crate) fn apply_locked(writes: &mut [FileWrite], context: &TaskContext) -> Result<(), String> {
    for write in writes.iter() {
        context.check()?;
        write
            .source
            .revalidate()
            .map_err(|_| format!("{} 已变化，请重新预览", write.source.path().display()))?;
    }
    let checkpoint = || {
        context.check().map_err(|message| AgentConfigurationError {
            kind: AgentConfigurationErrorKind::Canceled,
            message,
            diagnostics: Vec::new(),
        })
    };
    // Prepare every byte before replacing the first native file.
    let mut staged = writes
        .iter()
        .map(|write| {
            write
                .after
                .as_ref()
                .map(|after| StagedFile::prepare(&write.source, after, checkpoint))
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.message)?;
    for index in 0..staged.len() {
        let Some(stage) = &mut staged[index] else {
            continue;
        };
        stage
            .ensure_parent(checkpoint, || {})
            .map_err(|error| error.message)?;
        let parent = stage.target_parent().ok_or("目标目录不存在")?.to_path_buf();
        for stage in staged.iter_mut().flatten() {
            stage
                .advance_after_creation(&parent)
                .map_err(|error| error.message)?;
        }
        for write in writes.iter_mut() {
            write
                .source
                .reanchor_after_parent_creation(&parent)
                .map_err(|error| error.message)?;
        }
    }
    for (write, stage) in writes.iter_mut().zip(staged.iter_mut()) {
        context.check()?;
        context.item(&write.id, AgentMaintenanceItemState::Running, None);
        context.progress(format!(
            "正在处理 {}",
            write
                .source
                .path()
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ));
        let result: Result<(), String> = (|| {
            if let Some(stage) = stage {
                if write.executable.is_some() || write.mode.is_some() {
                    native_history::record_file(&write.source, write.after.as_deref())?;
                }
                let result = stage.commit(checkpoint).map_err(|error| error.message)?;
                if !matches!(result, crate::services::agent_cli::config_support::file_writer::CommitResult::Unchanged) { context.changed(); }
                #[cfg(unix)]
                if matches!(result, crate::services::agent_cli::config_support::file_writer::CommitResult::AppliedNotSynced) {
                    return Err("文件已替换，但目录同步未完成，请核对当前内容".to_owned());
                }
                #[cfg(not(unix))]
                let _ = result;
                stage.verify().map_err(|error| error.message)?;
                #[cfg(unix)]
                if write.executable.is_some() || write.mode.is_some() {
                    context.check()?;
                    set_permissions(&write.source, write.executable, write.mode)?;
                    context.changed();
                }
                #[cfg(not(unix))]
                let _ = (write.executable, write.mode);
            } else if let (Some(anchor), Some(bytes)) =
                (write.source.verified_anchor(), write.source.bytes())
            {
                removal::remove(anchor, Some(bytes), || {
                    context.check()?;
                    context.changed();
                    Ok(())
                })?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => context.item(&write.id, AgentMaintenanceItemState::Completed, None),
            Err(message) => {
                context.item(
                    &write.id,
                    AgentMaintenanceItemState::Failed,
                    Some(message.clone()),
                );
                return Err(message);
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_permissions(
    source: &GuardedFile,
    executable: Option<bool>,
    desired_mode: Option<u32>,
) -> Result<(), String> {
    use crate::services::agent_cli::environment::verified_path::inspect_verified_path;
    use std::os::unix::fs::PermissionsExt;
    let guard = inspect_verified_path(
        &[source.allowed_root()],
        source.allowed_root(),
        source.path(),
        AgentAssetSourceKind::File,
    )
    .map_err(|_| "权限目标已变化")?;
    let mode = guard
        .metadata()
        .map_err(|_| "权限不可读取")?
        .permissions()
        .mode();
    let next = desired_mode
        .map(|mode| mode & 0o777)
        .unwrap_or_else(|| match executable {
            Some(true) => mode | 0o100,
            Some(false) => mode & !0o111,
            None => mode,
        });
    guard
        .source_handle()
        .set_permissions(std::fs::Permissions::from_mode(next))
        .map_err(|_| "文件已保存，但执行权限设置失败")?;
    guard
        .revalidate_identity()
        .map_err(|_| "权限目标已变化".to_owned())
}
