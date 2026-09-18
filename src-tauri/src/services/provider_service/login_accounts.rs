use super::{MutationDecision, ProviderService};
use crate::{
    app_events::PROVIDERS_CHANGED_EVENT,
    models::{LoginAccount, LoginPlatform},
    services::{login_profiles, provider_browser_login},
    util::unix_millis,
};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::Emitter;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LinkedLoginProvider {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginAccountSummary {
    #[serde(flatten)]
    pub account: LoginAccount,
    pub platform_label: String,
    pub profile_present: bool,
    pub cookie_count: usize,
    pub busy: bool,
    pub can_open: bool,
    pub can_login: bool,
    pub session_label: String,
    pub session_problem: Option<String>,
    pub linked_providers: Vec<LinkedLoginProvider>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginCookieSummary {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub path: String,
    pub expires: f64,
    pub http_only: bool,
    pub secure: bool,
}

impl ProviderService<'_> {
    pub(crate) fn list_login_accounts(&self) -> Result<Vec<LoginAccountSummary>, String> {
        // Preserve the actual pre-management profile without inventing an identity
        // or assigning any previously imported station to it.
        let existing = login_profiles::directory(self.app, "profile")?;
        if existing.exists() {
            self.mutate_decided(|data| {
                if data.login_accounts.iter().any(|a| a.id == "profile") {
                    return Ok(MutationDecision::unchanged(()));
                }
                data.login_accounts.push(LoginAccount::new(
                    "profile".into(),
                    "之前的登录环境".into(),
                    LoginPlatform::Unknown,
                    unix_millis() as i64,
                ));
                Ok(MutationDecision::changed(()))
            })?;
        }
        let data = self.snapshot();
        data.login_accounts
            .iter()
            .cloned()
            .map(|account| {
                let observed = login_profiles::identity(self.app, &account.id)?;
                let session_problem =
                    verify_identity(&account, observed.platform, observed.identity.as_deref())
                        .err();
                let cookie_count = login_profiles::cookies(self.app, &account.id)?.len();
                let busy = provider_browser_login::account_busy(&account.id);
                let (session_label, can_login) = login_session_presentation(
                    account.identity.is_some(),
                    cookie_count,
                    busy,
                    session_problem.is_some(),
                );
                Ok(LoginAccountSummary {
                    platform_label: account.platform.label().into(),
                    profile_present: login_profiles::directory(self.app, &account.id)?.exists(),
                    cookie_count,
                    busy,
                    can_open: account.platform.account_url(false).is_some(),
                    can_login,
                    session_label: session_label.into(),
                    session_problem,
                    linked_providers: data
                        .providers
                        .iter()
                        .filter(|p| {
                            p.auth
                                .browser_binding
                                .as_ref()
                                .and_then(|b| b.account_id.as_deref())
                                == Some(account.id.as_str())
                        })
                        .map(|p| LinkedLoginProvider {
                            id: p.identity.id.clone(),
                            name: p.display_label(),
                        })
                        .collect(),
                    account,
                })
            })
            .collect()
    }

    pub(crate) fn create_login_account(
        &self,
        name: String,
        platform: LoginPlatform,
    ) -> Result<LoginAccount, String> {
        let name = account_name(&name)?;
        self.mutate_decided(|data| {
            if data.login_accounts.len() >= 100 {
                return Err("最多保存 100 个登录账号".into());
            }
            let account = LoginAccount::new(
                format!(
                    "account-{}-{}",
                    unix_millis() as i64,
                    SEQUENCE.fetch_add(1, Ordering::Relaxed)
                ),
                name,
                platform,
                unix_millis() as i64,
            );
            data.login_accounts.push(account.clone());
            Ok(MutationDecision::changed(account))
        })
    }

    pub(crate) fn update_login_account(
        &self,
        id: String,
        name: String,
        platform: LoginPlatform,
    ) -> Result<(), String> {
        let name = account_name(&name)?;
        if provider_browser_login::account_busy(&id) {
            return Err("请先结束该账号的登录任务".into());
        }
        self.mutate_decided(|data| {
            let account = data
                .login_accounts
                .iter_mut()
                .find(|a| a.id == id)
                .ok_or("登录账号已不存在")?;
            if account.platform != platform {
                if account.identity.is_some() {
                    return Err("已识别的平台不能更换，请新增登录账号".into());
                }
                account.platform = platform;
                account.generation += 1;
            }
            account.name = name;
            Ok(MutationDecision::changed(()))
        })
    }

    pub(crate) fn login_account(&self, id: &str) -> Result<LoginAccount, String> {
        self.ensure_storage_ready()?;
        self.snapshot()
            .login_accounts
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| "请选择已保存的登录账号".into())
    }

    pub(crate) fn check_login_account(
        &self,
        expected: &LoginAccount,
    ) -> Result<LoginAccount, String> {
        let current = self.login_account(&expected.id)?;
        if current.generation != expected.generation {
            return Err("账号登录状态已更改，本次任务已停止，请重试".into());
        }
        Ok(current)
    }

    pub(crate) fn record_login_account(&self, expected: &LoginAccount) -> Result<(), String> {
        let observation = login_profiles::identity(self.app, &expected.id)?;
        self.mutate_decided(|data| {
            let account = data
                .login_accounts
                .iter_mut()
                .find(|a| a.id == expected.id && a.generation == expected.generation)
                .ok_or("账号已更改，未保存旧登录结果")?;
            verify_identity(
                account,
                observation.platform,
                observation.identity.as_deref(),
            )?;
            if account.platform == LoginPlatform::Unknown
                && observation.platform != LoginPlatform::Unknown
            {
                account.platform = observation.platform;
            }
            if observation.identity.is_some() {
                account.identity = observation.identity;
                account.identity_observed_at = observation.observed_at;
            }
            account.last_opened_at = Some(unix_millis() as i64);
            Ok(MutationDecision::changed(()))
        })
    }

    pub(crate) fn login_account_cookies(
        &self,
        id: &str,
    ) -> Result<Vec<LoginCookieSummary>, String> {
        self.login_account(id)?;
        Ok(login_profiles::cookies(self.app, id)?
            .into_iter()
            .map(|c| LoginCookieSummary {
                id: c.id(),
                name: c.name,
                domain: c.domain,
                path: c.path,
                expires: c.expires,
                http_only: c.http_only,
                secure: c.secure,
            })
            .collect())
    }

    pub(crate) fn read_login_cookie(&self, id: &str, cookie_id: &str) -> Result<String, String> {
        self.login_account(id)?;
        login_profiles::cookies(self.app, id)?
            .into_iter()
            .find(|c| c.id() == cookie_id)
            .map(|c| c.value)
            .ok_or_else(|| "Cookie 已更新或不存在，请刷新列表".into())
    }

    pub(crate) fn remove_login_account(
        &self,
        id: String,
        remove_entry: bool,
    ) -> Result<(), String> {
        if provider_browser_login::account_busy(&id) {
            return Err("请先结束该账号的登录任务".into());
        }
        let _slot = provider_browser_login::idle_slot()?;
        let account = self.login_account(&id)?;
        let directory = login_profiles::directory(self.app, &id)?;
        let removed = directory.with_file_name(format!("removed-{id}-{}", unix_millis()));
        let existed = directory.exists();
        if existed {
            std::fs::rename(&directory, &removed).map_err(|_| "登录环境正在使用，暂时无法清除")?;
        }
        let result = self.mutate_decided(|data| {
            reset_account(data, &id, account.generation, remove_entry)?;
            Ok(MutationDecision::changed(()))
        });
        if result.is_err() && existed {
            let _ = std::fs::rename(&removed, &directory);
        }
        result?;
        if existed {
            std::fs::remove_dir_all(&removed).map_err(|_| "账号已移除，但本地登录目录清理失败")?;
        }
        let _ = self.app.emit(PROVIDERS_CHANGED_EVENT, ());
        Ok(())
    }
}

fn account_name(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 80 {
        return Err("请输入 1 至 80 个字的账号备注".into());
    }
    Ok(value.into())
}

fn login_session_presentation(
    identified: bool,
    cookie_count: usize,
    busy: bool,
    identity_problem: bool,
) -> (&'static str, bool) {
    if busy {
        ("登录任务进行中", false)
    } else if identity_problem {
        ("登录身份不一致，请重新登录原账号", false)
    } else if cookie_count > 0 {
        ("登录状态已保存，使用时验证", true)
    } else if identified {
        ("需要重新登录", true)
    } else {
        ("尚未登录", true)
    }
}

pub(super) fn verify_identity(
    account: &LoginAccount,
    platform: LoginPlatform,
    identity: Option<&str>,
) -> Result<(), String> {
    if platform != LoginPlatform::Unknown
        && account.platform != LoginPlatform::Unknown
        && account.platform != LoginPlatform::Other
        && platform != account.platform
    {
        return Err("实际登录平台与所选账号不一致，请新增或选择对应账号".into());
    }
    if let (Some(expected), Some(actual)) = (account.identity.as_deref(), identity) {
        if !expected.eq_ignore_ascii_case(actual) {
            return Err("实际登录身份与所选账号不一致，请新增另一个登录账号".into());
        }
    }
    Ok(())
}

fn reset_account(
    data: &mut crate::models::AppData,
    id: &str,
    generation: u64,
    remove_entry: bool,
) -> Result<(), String> {
    let entry = data
        .login_accounts
        .iter_mut()
        .find(|a| a.id == id && a.generation == generation)
        .ok_or("账号已变更，请刷新重试")?;
    // Forgetting a browser session does not change who owns this account or
    // its station bindings. Retain the identity so re-login still rejects B
    // when the user selected A; observation and use times remain historical.
    entry.generation += 1;
    if remove_entry {
        data.login_accounts.retain(|a| a.id != id);
        for provider in &mut data.providers {
            if let Some(binding) = &mut provider.auth.browser_binding {
                if binding.account_id.as_deref() == Some(id) {
                    // Detaching a browser does not invalidate a rotating business session.
                    binding.account_id = None;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        AppData, BrowserLoginBinding, BrowserLoginMechanism, Provider, ProviderInput,
    };
    #[test]
    fn clearing_a_preserves_b_and_all_station_credentials() {
        let mut data = AppData::default();
        for id in ["a", "b"] {
            let mut account = LoginAccount::new(id.into(), id.into(), LoginPlatform::LinuxDo, 1);
            account.identity = Some(format!("identity-{id}"));
            account.identity_observed_at = Some(2);
            account.last_opened_at = Some(3);
            account.last_used_at = Some(4);
            data.login_accounts.push(account);
            let mut provider = Provider::from_input(ProviderInput::default(), id.into());
            provider.auth.session_cookie = format!("session={id}");
            provider.auth.browser_binding = Some(BrowserLoginBinding {
                account_id: Some(id.into()),
                platform: LoginPlatform::LinuxDo,
                mechanism: BrowserLoginMechanism::Oauth,
                imported_at: 1,
            });
            data.providers.push(provider);
        }
        let original_auth = data
            .providers
            .iter()
            .map(|p| p.auth.clone())
            .collect::<Vec<_>>();
        reset_account(&mut data, "a", 0, false).unwrap();
        assert_eq!(data.login_accounts[0].generation, 1);
        let retained_a = &data.login_accounts[0];
        assert_eq!(retained_a.identity.as_deref(), Some("identity-a"));
        assert_eq!(retained_a.identity_observed_at, Some(2));
        assert_eq!(retained_a.last_opened_at, Some(3));
        assert_eq!(retained_a.last_used_at, Some(4));
        assert!(verify_identity(retained_a, LoginPlatform::LinuxDo, Some("identity-b")).is_err());
        assert!(verify_identity(retained_a, LoginPlatform::LinuxDo, Some("identity-a")).is_ok());
        assert_eq!(
            data.login_accounts[1].identity.as_deref(),
            Some("identity-b")
        );
        assert_eq!(
            data.providers
                .iter()
                .map(|p| p.auth.clone())
                .collect::<Vec<_>>(),
            original_auth
        );
        assert!(reset_account(&mut data, "a", 0, true).is_err());
        let request = super::super::ProviderRequestContext::capture(&data.providers[0]);
        reset_account(&mut data, "a", 1, true).unwrap();
        assert!(data.providers[0]
            .auth
            .browser_binding
            .as_ref()
            .unwrap()
            .account_id
            .is_none());
        assert_eq!(data.providers[1].auth, original_auth[1]);
        assert_eq!(data.providers[0].auth.session_cookie, "session=a");
        assert!(
            request.matches(&data.providers[0]),
            "detaching must allow an in-flight rotation to persist"
        );
    }
    #[test]
    fn known_identity_and_platform_mismatches_are_rejected() {
        let mut account = LoginAccount::new("a".into(), "A".into(), LoginPlatform::LinuxDo, 1);
        account.identity = Some("platform-a".into());
        assert!(verify_identity(&account, LoginPlatform::LinuxDo, Some("platform-b")).is_err());
        assert!(verify_identity(&account, LoginPlatform::Github, Some("platform-a")).is_err());
        assert!(verify_identity(&account, LoginPlatform::LinuxDo, None).is_ok());
    }

    #[test]
    fn login_status_distinguishes_retained_identity_from_saved_session() {
        assert_eq!(
            login_session_presentation(true, 0, false, false),
            ("需要重新登录", true)
        );
        assert_eq!(
            login_session_presentation(false, 0, false, false),
            ("尚未登录", true)
        );
        assert_eq!(
            login_session_presentation(true, 5, false, false),
            ("登录状态已保存，使用时验证", true)
        );
        assert!(!login_session_presentation(true, 5, true, false).1);
        assert!(!login_session_presentation(true, 5, false, true).1);
    }
}
