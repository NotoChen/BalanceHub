mod publish;
mod tree;
use crate::{
    models::*,
    services::{
        agent_cli::{
            self,
            config_support::directories,
            contracts::SessionReadBudget,
            environment::mutation::{locking::shared_locks, GuardedDirectory},
            maintenance::{self, MaintenanceService},
        },
        cli_sessions::workbench::{self, AgentSessionActor},
    },
};
use std::path::{Component, Path, PathBuf};
use tauri::{AppHandle, Emitter};

pub(crate) fn plan(
    app: &AppHandle,
    window: &str,
    actor: &str,
    service: &MaintenanceService,
    request: &AgentSessionRelocationRequest,
    budget: &SessionReadBudget,
) -> Result<AgentMaintenancePlan, String> {
    budget.check()?;
    let (target, locator) = workbench::relocation_source(
        AgentSessionActor {
            app,
            window_label: window,
        },
        &request.scope_revision,
        &request.session_ref,
    )?;
    let adapter = agent_cli::definition(target.cli_kind)
        .sessions()
        .and_then(|adapter| adapter.history().relocation)
        .ok_or("此 Agent 暂无项目迁移适配器")?;
    let root = target
        .config_root
        .canonicalize()
        .map_err(|_| "会话来源不可访问")?;
    let (from, to, destination_workdir) = destination(&target, &request.destination)?;
    let locator = locator.canonicalize().map_err(|_| "所选会话已不可访问")?;
    let paths = (adapter.paths)(&root, &locator, &from, &to)?;
    require_inactive(&target, &destination_workdir)?;
    let destination_guard =
        GuardedDirectory::capture_path(&paths.destination).map_err(|error| error.message)?;
    if !matches!(destination_guard, GuardedDirectory::Missing { .. }) {
        return Err("新目录已经有此 Agent 的项目历史，请先处理目标记录后再迁移".to_owned());
    }
    let project_guard =
        GuardedDirectory::capture_path(&destination_workdir).map_err(|error| error.message)?;
    if !matches!(project_guard, GuardedDirectory::Present(_)) {
        return Err("请选择已存在的新项目目录".to_owned());
    }
    let snapshot = tree::scan(&root, &paths, adapter, &|| budget.check())?;
    publish::require_space(&root, snapshot.output_bytes)?;
    let title = format!(
        "迁移 {} 的项目历史",
        agent_cli::definition(target.cli_kind).label
    );
    let changes = vec![AgentAssetPlanChange {
        label: "项目目录与历史位置".to_owned(),
        path: None,
        before: Some(format!(
            "项目目录：{}\n历史目录：{}",
            paths.from,
            display(&paths.source)
        )),
        after: Some(format!(
            "项目目录：{}\n历史目录：{}",
            paths.to,
            display(&paths.destination)
        )),
    }];
    let mut preview = maintenance::preview(
        AgentMaintenanceKind::SessionRelocation,
        title,
        changes,
        vec![
            maintenance::item("prepare", "准备迁移文件"),
            maintenance::item("verify", "核对原始内容"),
            maintenance::item("publish", "迁移并保留原始备份"),
        ],
    );
    preview.notes = vec![format!("{} 个主会话，{} 个文件，共 {:.1} MiB；包括子会话、工具结果和项目记忆。", snapshot.sessions, snapshot.files.len(), snapshot.bytes as f64 / 1024.0 / 1024.0),
        "保留会话 ID 与消息原文，只修改顶层 cwd 及会话索引中的目录字段。".to_owned(),
        "请关闭这个项目的 Agent 与编辑器会话。原始文件会保存到会话扫描目录外，任务详情提供备份位置。".to_owned()];
    let bytes = snapshot.files.iter().fold(0usize, |total, file| {
        total
            .saturating_add(file.relative.as_os_str().len())
            .saturating_add(512)
    });
    let app = app.clone();
    let window = window.to_owned();
    let request = request.clone();
    budget.check()?;
    service.plan(actor, preview, bytes, move |context| {
        let locks = shared_locks();
        let domains = vec![format!("session-project:{}", display(&paths.source)), format!("session-project:{}", display(&paths.destination))];
        let _lock = locks.acquire(&domains, context.canceled(), context.deadline()).map_err(|_| "等待项目迁移超时或已取消")?;
        context.check()?;
        let (fresh, _) = workbench::relocation_source(AgentSessionActor { app: &app, window_label: &window }, &request.scope_revision, &request.session_ref)?;
        if fresh.config_root.canonicalize().ok().as_ref() != Some(&root) || fresh.source_identity != target.source_identity { return Err("会话来源在预览后发生变化".to_owned()); }
        require_inactive(&fresh, &destination_workdir)?;
        destination_guard.revalidate().map_err(|_| "目标历史目录已出现，请重新预览")?;
        project_guard.revalidate().map_err(|_| "新项目目录已变化")?;
        publish::require_space(&root, snapshot.output_bytes)?;
        let backup = root.join("balancehub-session-relocations").join(context.id());
        let staged = backup.join("staged"); let original = backup.join("original");
        let backup_guard = publish::create_backup(&root, &backup)?;
        let mut moved = false;
        let result = (|| {
            directories::ensure_directory_before(&root, &staged, || context.check())?;
            publish::manifest(&root, &backup, &serde_json::json!({ "from": paths.source, "to": paths.destination, "projectFrom": paths.from, "projectTo": paths.to, "original": original, "staged": staged, "files": snapshot.files.len(), "sessions": snapshot.sessions, "createdAt": chrono::Local::now().to_rfc3339() }))?;
            context.item("prepare", AgentMaintenanceItemState::Running, None);
            tree::stage(&snapshot, &root, &staged, &paths, adapter, context)?;
            context.item("prepare", AgentMaintenanceItemState::Completed, None);
            context.item("verify", AgentMaintenanceItemState::Running, None);
            tree::verify(&snapshot, &paths, &|| context.check())?;
            require_inactive(&target, &destination_workdir)?;
            project_guard.revalidate().map_err(|_| "新项目目录发生变化")?;
            destination_guard.revalidate().map_err(|_| "目标历史目录已出现")?;
            context.item("verify", AgentMaintenanceItemState::Completed, None);
            context.begin_commit("正在切换项目目录，原始内容已准备备份")?;
            let first_synced = publish::move_directory(&root, &paths.source, &original)?;
            moved = true;
            context.changed();
            let second_synced = match publish::move_directory(&root, &staged, &paths.destination) {
                Ok(synced) => synced,
                Err(error) => {
                    return match publish::move_directory(&root, &original, &paths.source) {
                        Ok(_) => Err(format!("{error}；原目录已还原")),
                        Err(restore) => Err(format!("{error}；自动还原失败：{restore}。原始内容保留在 {}", display(&original))),
                    };
                }
            };
            context.item("publish", AgentMaintenanceItemState::Completed, Some(format!("原始备份：{}。会话 ID 保持不变。", display(&original))));
            if !first_synced || !second_synced { return Err("目录已迁移，但磁盘目录同步未完成，请核对新目录和备份".to_owned()); }
            Ok(())
        })();
        if moved {
            workbench::invalidate_source(&target.config_root);
            let _ = app.emit("agent-session-sources-changed", ());
        }
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                let cleanup = publish::clean_unpublished(&backup_guard, &staged, &original).err();
                Err(format!("{error}{}；迁移记录：{}", cleanup.map(|message| format!("；{message}")).unwrap_or_default(), display(&backup)))
            }
        }
    })
}

fn destination(
    target: &workbench::ResolvedSessionTarget,
    input: &str,
) -> Result<(String, String, PathBuf), String> {
    let input = input.trim();
    if input.is_empty() || input.chars().any(char::is_control) {
        return Err("请输入新项目目录的绝对路径".to_owned());
    }
    let (from, path) = match &target.runtime_scope {
        AgentRuntimeScope::Wsl { distro_id } => {
            let distro = crate::services::wsl::require_running(distro_id)?;
            let (name, from) = crate::services::wsl::split_host_path(&target.workdir)
                .ok_or("原项目缺少 WSL 路径")?;
            if !name.eq_ignore_ascii_case(&distro.name) {
                return Err("原项目不属于选定 WSL 环境".to_owned());
            }
            (from, crate::services::wsl::host_path(&distro.name, input)?)
        }
        AgentRuntimeScope::Native => (display(&target.workdir), PathBuf::from(input)),
    };
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("请使用不包含相对层级的新项目绝对路径".to_owned());
    }
    let path = path
        .canonicalize()
        .map_err(|_| "新项目目录不存在或不可访问")?;
    if !path.is_dir() {
        return Err("新项目路径不是目录".to_owned());
    }
    let to = match &target.runtime_scope {
        AgentRuntimeScope::Wsl { distro_id } => {
            let distro = crate::services::wsl::running(distro_id)?;
            let (name, guest) =
                crate::services::wsl::split_host_path(&path).ok_or("新目录不是同一 WSL 环境")?;
            if !name.eq_ignore_ascii_case(&distro.name) {
                return Err("会话只能在同一 WSL 发行版内迁移".to_owned());
            }
            guest
        }
        AgentRuntimeScope::Native => display(&path),
    };
    Ok((from, to, path))
}
fn require_inactive(
    target: &workbench::ResolvedSessionTarget,
    destination: &Path,
) -> Result<(), String> {
    for instance in crate::services::cli_runtime::runtime_instances() {
        if instance.cli_kind == target.cli_kind
            && instance.status == TemporaryCliInstanceStatus::Running
        {
            let workdir = Path::new(&instance.workdir);
            if workdir.starts_with(&target.workdir) || workdir.starts_with(destination) {
                return Err("这个项目仍有运行中的 Agent，请关闭后再迁移".to_owned());
            }
        }
    }
    if let AgentRuntimeScope::Wsl { distro_id } = &target.runtime_scope {
        crate::services::wsl::require_running(distro_id)?;
    }
    Ok(())
}
fn display(path: &Path) -> String {
    crate::platform::paths::compatible_path(path)
        .to_string_lossy()
        .into_owned()
}
