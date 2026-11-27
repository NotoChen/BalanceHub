use super::*;
use crate::models::ProviderInput;

pub fn start_account(id: String, authorizations: bool) -> Result<ProviderBrowserLoginTask, String> {
    let account = ProviderService::new().login_account(&id)?;
    let url = account
        .platform
        .account_url(authorizations)
        .ok_or("请先选择 Linux DO 或 GitHub；其他账号从站点的登录并导入进入")?;
    if !browser_runtime::status(true)?.ready {
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
    emit(&task);
    let run_id = task.run_id.clone();
    crate::runtime::spawn(async move {
        // 与站点登录一致：后台任务异常退出时发布失败终态，而不是让账号
        // 窗口任务永远停在「进行中」。
        use futures_util::FutureExt;
        let result = std::panic::AssertUnwindSafe(run_account(&run_id, account, url, cancelled))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                log::error!("账号窗口任务异常退出（{run_id}）");
                Err(LoginError::failed("账号窗口异常退出，请重新打开"))
            });
        match result {
            Ok(()) => publish(&run_id, "completed", "账号窗口已关闭，本地登录状态已保存"),
            Err(error) => publish(
                &run_id,
                if error.is_cancelled() {
                    "cancelled"
                } else {
                    "failed"
                },
                error.message(),
            ),
        }
    });
    Ok(task)
}

async fn run_account(
    run_id: &str,
    account: LoginAccount,
    url: &str,
    mut cancelled: watch::Receiver<bool>,
) -> Result<(), LoginError> {
    let _slot = tokio::select! {
        biased;
        _ = cancelled.changed() => return Err(LoginError::cancelled("账号窗口已取消")),
        slot = browser_profiles::acquire(ProfileKey::Account(account.id.clone())) => slot?,
    };
    if *cancelled.borrow() {
        return Err(LoginError::cancelled("账号窗口已取消"));
    }
    let account = ProviderService::new().check_login_account(&account)?;
    let runtime = browser_runtime::acquire().await?;
    let settings = crate::host::core_state()
        .data
        .read()
        .map_err(|_| "读取代理设置失败")?
        .settings
        .clone();
    let provider = Provider::from_input(ProviderInput::default(), "login-account".into());
    let proxy = network::resolve_proxy(&settings, &provider)
        .browser(&reqwest::Url::parse(url).map_err(|_| "平台地址无效")?)?;
    let profile = crate::services::login_profiles::directory(&account.id)?;
    publish(run_id, "opening", "正在打开所选账号的独立窗口");
    let progress_id = run_id.to_string();
    let mut session = BrowserSession::spawn(
        &runtime.directory,
        Arc::new(move |phase| {
            if phase == CheckInPhase::WaitingHuman {
                publish(
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
        _ = cancelled.changed() => Err(LoginError::cancelled("账号窗口已取消，本地登录状态已保留")),
        result = session.request("account", json!({ "url": url, "accountName": account.name, "profileDir": profile,
            "proxy": proxy, "executablePath": runtime.browser, "expectedPlatform": account.platform, "expectedIdentity": account.identity, "timeoutMs": 600_000 })) => result.map(|_| ()).map_err(LoginError::from),
    };
    session.close().await;
    ProviderService::new().record_login_account(&account)?;
    result
}
