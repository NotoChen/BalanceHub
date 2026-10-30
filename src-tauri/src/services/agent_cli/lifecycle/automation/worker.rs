use super::super::{usage, LifecycleService};
use super::AUTOMATIC_ACTOR;
use crate::{models::*, services::cli_paths::user_home, state::AppState};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::Manager;

const TICK: Duration = Duration::from_secs(30);
const CHECK_TIMEOUT: Duration = Duration::from_secs(60);
const EMPTY_INVENTORY_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

pub(crate) fn start_worker(app: &tauri::AppHandle) {
    let service = app.state::<AppState>().agent_lifecycle();
    if !service.attach_worker(app.clone()) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let mut recovered = false;
        let mut settings_signature = String::new();
        let mut last_scan = None::<Instant>;
        let mut retry_at = Instant::now();
        loop {
            let state = app.state::<AppState>();
            if state.load_error().is_some() {
                service.wait_for_work(TICK).await;
                continue;
            }
            let settings = state
                .data
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .settings
                .clone();
            let Some(home) = user_home().and_then(|path| path.canonicalize().ok()) else {
                service.wait_for_work(TICK).await;
                continue;
            };
            if !recovered {
                let recovery_service = Arc::clone(&service);
                let recovery_settings = settings.clone();
                let recovery_home = home.clone();
                let recovery = tokio::time::timeout(
                    CHECK_TIMEOUT,
                    tauri::async_runtime::spawn_blocking(move || {
                        recovery_service.reconcile_history(recovery_settings, recovery_home)
                    }),
                )
                .await;
                recovered = matches!(recovery, Ok(Ok(Ok(()))));
                if !recovered {
                    service
                        .automatic()
                        .block("升级记录核对失败，自动更新已暂停；请检查任务记录并重启应用".into());
                    service.publish_automatic();
                    service.wait_for_work(TICK).await;
                    continue;
                }
            }
            let signature = serde_json::json!([
                settings.agent_cli_paths,
                settings.proxy_mode,
                settings.proxy_url
            ])
            .to_string();
            let cached = service.cached_catalog();
            let due = cached
                .as_ref()
                .and_then(|catalog| catalog.next_check_at.as_deref())
                .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
                .is_some_and(|time| time <= chrono::Utc::now());
            let dirty = service.take_installation_change();
            let discover = cached.is_none()
                || signature != settings_signature
                || due
                || dirty
                || last_scan.is_none_or(|time| time.elapsed() >= EMPTY_INVENTORY_INTERVAL);
            if discover && Instant::now() >= retry_at {
                let checked = tokio::time::timeout(
                    CHECK_TIMEOUT,
                    service.catalog(
                        settings.clone(),
                        home.clone(),
                        AgentLifecycleCatalogRequest {
                            agent_kind: None,
                            version_refresh: AgentLifecycleVersionRefresh::IfStale,
                        },
                    ),
                )
                .await;
                match checked {
                    Ok(Ok(_)) => {
                        settings_signature = signature;
                        last_scan = Some(Instant::now());
                    }
                    _ => {
                        retry_at = Instant::now() + Duration::from_secs(60);
                        service.invalidate_installations();
                        if let Some(catalog) = cached {
                            for entry in catalog
                                .automatic_updates
                                .iter()
                                .filter(|entry| entry.enabled)
                            {
                                if let Some(key) = &entry.key {
                                    service.automatic().observe(
                                        key,
                                        AgentAutomaticUpdateState::Scheduled,
                                        "安装或版本检查未完成，稍后重新检查",
                                    );
                                }
                            }
                            service.publish_automatic();
                        }
                        service.wait_for_work(TICK).await;
                        continue;
                    }
                }
            } else if discover {
                service.invalidate_installations();
                service.wait_for_work(TICK).await;
                continue;
            }
            if !finish_history(&service).await {
                service.wait_for_work(TICK).await;
                continue;
            }
            if let Some(catalog) = service.cached_catalog() {
                for target in catalog.targets {
                    let Some(key) = target.update_key.as_deref() else {
                        continue;
                    };
                    let Some(generation) = service.automatic().generation(key) else {
                        continue;
                    };
                    if !catalog.automatic_updates.iter().any(|entry| {
                        entry.target_id.as_deref() == Some(&target.id) && entry.can_enable
                    }) {
                        continue;
                    }
                    if target.version.state != AgentLifecycleVersionState::UpdateAvailable
                        || target.version.stale
                    {
                        let message =
                            target.version.message.clone().unwrap_or_else(|| {
                                match target.version.state {
                                    AgentLifecycleVersionState::UpToDate => {
                                        "当前已是此渠道的最新版本".into()
                                    }
                                    AgentLifecycleVersionState::AheadOfLatest => {
                                        "当前版本高于此渠道，自动更新不会降级".into()
                                    }
                                    _ => "等待下一次版本检查".into(),
                                }
                            });
                        service.automatic().observe(
                            key,
                            AgentAutomaticUpdateState::Scheduled,
                            message,
                        );
                        continue;
                    }
                    if service.has_active_update(key) {
                        service.automatic().observe(
                            key,
                            AgentAutomaticUpdateState::WaitingForIdle,
                            "等待此安装正在执行的升级任务完成",
                        );
                        continue;
                    }
                    update_one(&service, &settings, &home, target, generation).await;
                    // A fresh installation snapshot is required after any attempted update.
                    if service.take_installation_change() {
                        service.invalidate_installations();
                        break;
                    }
                }
                service.publish_automatic();
            }
            service.wait_for_work(TICK).await;
        }
    });
}

async fn update_one(
    service: &Arc<LifecycleService>,
    settings: &AppSettings,
    home: &std::path::Path,
    target: AgentLifecycleTarget,
    generation: u64,
) {
    let Some(scope) = usage::UpdateScope::from_target(&target) else {
        return;
    };
    let key = scope.key.clone();
    let reservation = tokio::time::timeout(
        Duration::from_secs(10),
        tauri::async_runtime::spawn_blocking(move || {
            let reservation = usage::reserve(&scope)?;
            scope.check_processes()?;
            Ok::<_, String>(reservation)
        }),
    )
    .await;
    let reservation = match reservation {
        Ok(Ok(Ok(reservation))) => reservation,
        result => {
            let message = match result {
                Ok(Ok(Err(message))) => message,
                _ => "检查进程占用超时，等待下次核对".into(),
            };
            service
                .automatic()
                .observe(&key, AgentAutomaticUpdateState::WaitingForIdle, message);
            service.publish_automatic();
            return;
        }
    };
    service.automatic().observe(
        &key,
        AgentAutomaticUpdateState::Scheduled,
        "正在核对安装和更新命令",
    );
    service.publish_automatic();
    let plan = tokio::time::timeout(
        CHECK_TIMEOUT,
        service.plan(
            AUTOMATIC_ACTOR,
            settings.clone(),
            PathBuf::from(home),
            AgentLifecyclePlanRequest {
                agent_kind: target.agent_kind,
                target_id: target.id,
                action: AgentLifecycleActionKind::Upgrade,
                expected_evidence_revision: target.evidence_revision,
            },
        ),
    )
    .await;
    let plan = match plan {
        Ok(Ok(plan)) if plan.update_key.as_deref() == Some(&key) => plan,
        Ok(Err(error))
            if error.kind == AgentLifecycleErrorKind::TargetChanged
                || error.reason == Some(AgentLifecycleUnavailableReason::AlreadyCurrent) =>
        {
            service.invalidate_installations();
            return;
        }
        result => {
            let message = match result {
                Ok(Err(error)) => error.message,
                _ => "更新计划未完成，请重新检测后恢复自动更新".into(),
            };
            pause_policy(service, &key, generation, message).await;
            return;
        }
    };
    if service.automatic().generation(&key) != Some(generation) {
        return;
    }
    let starter = Arc::clone(service);
    let request = AgentLifecycleApplyRequest {
        plan_token: plan.plan_token,
        agent_kind: plan.agent_kind,
        target_id: plan.target_id,
        action: plan.action,
    };
    let operation = match tauri::async_runtime::spawn_blocking(move || {
        starter.start(AUTOMATIC_ACTOR, &request)
    })
    .await
    {
        Ok(Ok(operation)) => operation,
        result => {
            service.invalidate_installations();
            let message = match result {
                Ok(Err(error)) => error.message,
                _ => "自动更新启动任务异常".into(),
            };
            pause_policy(service, &key, generation, message).await;
            return;
        }
    };
    let automatic = service.automatic();
    let operation_copy = operation.clone();
    let policy_key = key.clone();
    if !matches!(
        tauri::async_runtime::spawn_blocking(move || automatic.begin(
            &policy_key,
            generation,
            &operation_copy
        ))
        .await,
        Ok(Ok(()))
    ) {
        let _ = service.cancel(AUTOMATIC_ACTOR, &operation.id);
        return;
    }
    service.automatic().observe(
        &key,
        AgentAutomaticUpdateState::Updating,
        "正在后台更新，可在任务中心查看命令和日志",
    );
    service.publish_automatic();
    let runner = Arc::clone(service);
    let operation_id = operation.id.clone();
    let running = tauri::async_runtime::spawn_blocking(move || {
        let _reservation = reservation;
        runner.run_operation(&operation_id);
    });
    if tokio::time::timeout(Duration::from_secs(7 * 60), running)
        .await
        .is_err()
    {
        pause_policy(
            service,
            &key,
            generation,
            "后台更新仍未确认结束，请查看任务记录；不会重复执行".into(),
        )
        .await;
    }
    finish_history(service).await;
    service.publish_automatic();
}

async fn finish_history(service: &Arc<LifecycleService>) -> bool {
    let automatic = service.automatic();
    let operations = match service.operations(AUTOMATIC_ACTOR) {
        Ok(operations) => operations,
        Err(error) => {
            automatic.block(format!(
                "升级记录不可用：{}；自动更新已停止，请修复后重启应用",
                error.message
            ));
            service.publish_automatic();
            return false;
        }
    };
    let result =
        tauri::async_runtime::spawn_blocking(move || automatic.recover_and_finish(&operations))
            .await;
    match result {
        Ok(Ok(changed)) => {
            if changed {
                service.publish_automatic();
            }
            true
        }
        result => {
            let message = match result {
                Ok(Err(message)) => message,
                _ => "升级记录保存任务未完成".into(),
            };
            service
                .automatic()
                .block(format!("{message}；自动更新已停止，请修复后重启应用"));
            service.publish_automatic();
            false
        }
    }
}

async fn pause_policy(
    service: &Arc<LifecycleService>,
    key: &str,
    generation: u64,
    message: String,
) {
    let automatic = service.automatic();
    let key = key.to_owned();
    if !matches!(
        tauri::async_runtime::spawn_blocking(move || automatic.pause(&key, generation, message))
            .await,
        Ok(Ok(()))
    ) {
        service
            .automatic()
            .block("自动更新暂停状态保存失败；本次运行已停止自动更新，请修复后重启应用".into());
    }
    service.publish_automatic();
}
