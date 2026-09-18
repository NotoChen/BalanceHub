use super::*;
use crate::models::ProviderInput;

pub(crate) fn start_account(
    app: &AppHandle,
    id: String,
    authorizations: bool,
) -> Result<ProviderBrowserLoginTask, String> {
    let account = ProviderService::new(app).login_account(&id)?;
    let url = account
        .platform
        .account_url(authorizations)
        .ok_or("请先选择 Linux DO 或 GitHub；其他账号从站点的登录并导入进入")?;
    if !browser_runtime::status(app, true)?.ready {
        return Err("请先安装浏览器组件".into());
    }
    let (cancel, cancelled) = watch::channel(false);
    let task = {
        let mut runs = runs().lock().map_err(|_| "创建账号任务失败")?;
        if runs
            .values()
            .any(|run| run.task.login_account_id == id && run.task.finished_at.is_none())
        {
            return Err("该账号已有登录任务，请先完成或取消".into());
        }
        if runs
            .values()
            .filter(|run| run.task.finished_at.is_none())
            .count()
            >= 10
        {
            return Err("请先完成已有登录任务".into());
        }
        let task = ProviderBrowserLoginTask {
            run_id: format!(
                "login-account-{}-{}",
                unix_millis(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
            provider_id: None,
            provider_name: account.name.clone(),
            login_account_id: id,
            operation: "account".into(),
            phase: "queued".into(),
            message: "等待打开账号窗口".into(),
            started_at: unix_millis() as u64,
            finished_at: None,
            error: None,
            can_cancel: true,
            can_show_window: false,
        };
        runs.insert(
            task.run_id.clone(),
            LoginRun {
                task: task.clone(),
                cancel,
                window: None,
            },
        );
        task
    };
    emit(app, &task);
    let app = app.clone();
    let run_id = task.run_id.clone();
    tauri::async_runtime::spawn(async move {
        let result = run_account(&app, &run_id, account, url, cancelled).await;
        match result {
            Ok(()) => publish(
                &app,
                &run_id,
                "completed",
                "账号窗口已关闭，本地登录状态已保存",
            ),
            Err(message) => publish(
                &app,
                &run_id,
                if message.contains("取消") {
                    "cancelled"
                } else {
                    "failed"
                },
                &message,
            ),
        }
    });
    Ok(task)
}

async fn run_account(
    app: &AppHandle,
    run_id: &str,
    account: LoginAccount,
    url: &str,
    mut cancelled: watch::Receiver<bool>,
) -> Result<(), String> {
    let _slot = tokio::select! {
        biased;
        _ = cancelled.changed() => return Err("账号窗口已取消".into()),
        slot = SLOT.acquire() => slot.map_err(|_| "登录队列不可用")?,
    };
    if *cancelled.borrow() {
        return Err("账号窗口已取消".into());
    }
    let account = ProviderService::new(app).check_login_account(&account)?;
    let runtime = browser_runtime::acquire(app).await?;
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .map_err(|_| "读取代理设置失败")?
        .settings
        .clone();
    let provider = Provider::from_input(ProviderInput::default(), "login-account".into());
    let proxy = network::resolve_proxy(&settings, &provider)
        .browser(&reqwest::Url::parse(url).map_err(|_| "平台地址无效")?)?;
    let profile = crate::services::login_profiles::directory(app, &account.id)?;
    publish(app, run_id, "opening", "正在打开所选账号的独立窗口");
    let progress_app = app.clone();
    let progress_id = run_id.to_string();
    let mut session = BrowserSession::spawn(
        &runtime.directory,
        Arc::new(move |phase| {
            if phase == CheckInPhase::WaitingHuman {
                publish(
                    &progress_app,
                    &progress_id,
                    "waitingLogin",
                    "登录或管理授权后关闭小窗，登录状态自动保存",
                );
            }
        }),
    )?;
    attach_window(run_id, session.window_control())?;
    let result = tokio::select! {
        biased;
        _ = cancelled.changed() => Err("账号窗口已取消，本地登录状态已保留".to_string()),
        result = session.request("account", json!({ "url": url, "accountName": account.name, "profileDir": profile,
            "proxy": proxy, "executablePath": runtime.browser, "expectedPlatform": account.platform, "expectedIdentity": account.identity, "timeoutMs": 600_000 })) => result.map(|_| ()).map_err(|e| e.to_string()),
    };
    session.close().await;
    ProviderService::new(app).record_login_account(&account)?;
    result
}
