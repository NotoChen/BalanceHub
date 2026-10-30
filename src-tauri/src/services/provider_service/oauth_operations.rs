use super::{find_provider, MutationDecision, ProviderService};
use crate::{
    adapters::cli_proxy_api,
    contracts::{OAuthAccountDetails, OAuthExportResult},
    models::*,
    services::oauth_accounts,
};
use std::{fs, io::Write, path::Path};

impl ProviderService<'_> {
    pub(crate) async fn oauth_details(&self, id: String) -> Result<OAuthAccountDetails, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        let oauth = provider
            .auth
            .oauth
            .as_ref()
            .ok_or("此卡片不是 OAuth 账号")?;
        let owner = oauth_accounts::device_id(self.app)?;
        let export_options = oauth_accounts::interchange::options(&provider.auth);
        Ok(OAuthAccountDetails {
            refreshable: !provider.auth.refresh_token.is_empty(),
            renewal_enabled_here: oauth.refresh_owner.as_deref() == Some(&owner),
            renewal_elsewhere: oauth.refresh_owner.as_ref().is_some_and(|o| o != &owner),
            export_options,
            provider: provider.into(),
        })
    }

    pub(crate) async fn update_oauth_metadata(
        &self,
        id: String,
        name: String,
        remark: String,
        refresh_interval: u64,
    ) -> Result<Provider, String> {
        if name.trim().is_empty()
            || name.chars().count() > 100
            || remark.chars().count() > crate::limits::MAX_PROVIDER_REMARK_CHARS
        {
            return Err("名称不能为空且最多 100 字，备注最多 160 字".into());
        }
        if refresh_interval != 0 && !(60..=86400).contains(&refresh_interval) {
            return Err("刷新间隔应为 60 至 86400 秒，或使用全局设置".into());
        }
        let result_id = id.clone();
        self.mutate_decided_async(move |data| {
            let provider = data
                .providers
                .iter_mut()
                .find(|p| p.identity.id == id && p.auth.oauth.is_some())
                .ok_or("OAuth 账号不存在")?;
            provider.identity.name = name.trim().into();
            provider.identity.remark = remark.trim().into();
            provider.automation.refresh_interval = refresh_interval;
            Ok(MutationDecision::changed(()))
        })
        .await?;
        self.providers_by_ids_async(&[result_id])
            .await?
            .into_iter()
            .next()
            .ok_or("账号已移除".into())
    }

    /// The stored credential exactly as BalanceHub holds it, for the in-app
    /// viewer. No file is written and no renewal state changes.
    pub(crate) async fn oauth_credential_source(&self, id: String) -> Result<String, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        oauth_accounts::interchange::source(&provider)
    }

    /// Replaces the stored tokens from an edited balancehub-oauth document.
    /// Account identity may be filled in but never repointed, so a pasted file
    /// for another account cannot silently take over this card.
    pub(crate) async fn replace_oauth_credential(
        &self,
        id: String,
        content: String,
        revision: u64,
    ) -> Result<Provider, String> {
        let lock = oauth_accounts::account_lock(&id);
        let _guard = lock.lock().await;
        let data = self.load_app_data_async().await?;
        let current = find_provider(&data, &id)?;
        if current.auth.credential_revision != revision {
            return Err("账号授权已变更，请重新打开后再保存".into());
        }
        let vendor = current
            .auth
            .oauth
            .as_ref()
            .ok_or("此卡片不是 OAuth 账号")?
            .vendor;
        let parsed = oauth_accounts::interchange::parse_file(
            &OAuthImportFile {
                name: "credential.json".into(),
                content,
            },
            Some(vendor),
        )?;
        let mut accepted = Vec::new();
        let mut problems = Vec::new();
        for entry in parsed {
            match entry {
                Ok(auth) => accepted.push(auth),
                Err(error) => problems.push(error),
            }
        }
        if !problems.is_empty() {
            return Err(format!("授权原文无效：{}", problems.join("；")));
        }
        if accepted.len() != 1 {
            return Err("授权原文应只包含当前这一个账号".into());
        }
        let next = accepted.pop().expect("checked");
        {
            let before = current.auth.oauth.as_ref().expect("checked");
            let after = next.oauth.as_ref().expect("parsed");
            for (label, old, new) in [
                ("账号 ID", &before.account_id, &after.account_id),
                ("用户 ID", &before.subject, &after.subject),
            ] {
                if !old.is_empty() && !new.is_empty() && old != new {
                    return Err(format!(
                        "授权原文的{label}与当前账号不一致，如需更换账号请重新授权或导入新卡片"
                    ));
                }
            }
        }
        let result_id = id.clone();
        self.mutate_decided_async(move |data| {
            let provider = data
                .providers
                .iter_mut()
                .find(|p| p.identity.id == id && p.auth.oauth.is_some())
                .ok_or("OAuth 账号不存在")?;
            if provider.auth.credential_revision != revision {
                return Err("账号授权已变更，请重新打开后再保存".into());
            }
            let owner = provider
                .auth
                .oauth
                .as_ref()
                .and_then(|oauth| oauth.refresh_owner.clone());
            provider.auth.access_token = next.access_token;
            provider.auth.refresh_token = next.refresh_token;
            provider.auth.access_token_expires_at = next.access_token_expires_at;
            let mut identity = next.oauth.expect("parsed");
            identity.refresh_owner = owner;
            // The edited document is an explicit replacement: any interrupted
            // rotation it supersedes is no longer pending.
            identity.refresh_pending = false;
            identity.refresh_error = None;
            identity.updated_at = Some(chrono::Utc::now().timestamp());
            provider.auth.oauth = Some(identity);
            provider.auth.credential_revision = provider.auth.credential_revision.saturating_add(1);
            Ok(MutationDecision::changed(()))
        })
        .await?;
        self.providers_by_ids_async(&[result_id])
            .await?
            .into_iter()
            .next()
            .ok_or("账号已移除".into())
    }

    async fn pause_oauth_under_lock(&self, id: String) -> Result<Provider, String> {
        let result_id = id.clone();
        self.mutate_decided_async(move |data| {
            let provider = data
                .providers
                .iter_mut()
                .find(|p| p.identity.id == id)
                .ok_or("账号已移除")?;
            let oauth = provider
                .auth
                .oauth
                .as_mut()
                .ok_or("此卡片不是 OAuth 账号")?;
            if oauth.refresh_owner.is_none() {
                return Ok(MutationDecision::unchanged(()));
            }
            oauth.refresh_owner = None;
            provider.auth.credential_revision = provider.auth.credential_revision.saturating_add(1);
            Ok(MutationDecision::changed(()))
        })
        .await?;
        self.providers_by_ids_async(&[result_id])
            .await?
            .into_iter()
            .next()
            .ok_or("账号已移除".into())
    }

    pub(crate) async fn export_oauth_file(
        &self,
        id: String,
        format: OAuthExportFormat,
        path: String,
        pause_renewal: bool,
    ) -> Result<OAuthExportResult, String> {
        let app = self.app.clone();
        // Complete the file write and renewal decision after a frontend timeout.
        tauri::async_runtime::spawn(async move {
            let service = ProviderService::new(&app);
            let lock = oauth_accounts::account_lock(&id);
            let _guard = lock.lock().await;
            let provider = find_provider(&service.load_app_data_async().await?, &id)?;
            let file = oauth_accounts::interchange::export(&provider, format)?;
            let content = file.content;
            tauri::async_runtime::spawn_blocking(move || save_authorization_file(&path, &content))
                .await
                .map_err(|_| "授权文件写入任务异常")??;
            let (provider, warning) = if pause_renewal {
                match service.pause_oauth_under_lock(id).await {
                    Ok(provider) => (provider, None),
                    Err(_) => (
                        provider,
                        Some(
                            "文件已导出，但本机续期暂停失败。请在目标应用使用前手动暂停本机续期。"
                                .into(),
                        ),
                    ),
                }
            } else {
                (provider, None)
            };
            Ok(OAuthExportResult {
                renewal_paused: provider
                    .auth
                    .oauth
                    .as_ref()
                    .is_some_and(|o| o.refresh_owner.is_none()),
                provider: provider.into(),
                file_name: file.name,
                instructions: file.instructions,
                warning,
            })
        })
        .await
        .map_err(|_| "导出任务异常，请核对所选文件".to_string())?
    }

    pub(crate) async fn import_cpa_preview(
        &self,
        id: String,
        names: Vec<String>,
    ) -> Result<OAuthImportPreview, String> {
        if names.is_empty() || names.len() > 64 {
            return Err("请选择 1 至 64 份 CPA 授权".into());
        }
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        let files = cli_proxy_api::download_selected(&data.settings, &provider, names).await?;
        oauth_accounts::imports::prepare(files, None, &data.providers)
    }

    pub(crate) async fn transfer_oauth_to_cpa(
        &self,
        id: String,
        gateway_id: String,
    ) -> Result<OAuthExportResult, String> {
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let lock = oauth_accounts::account_lock(&id);
            let _guard = lock.lock().await;
            let service = ProviderService::new(&app);
            let data = service.load_app_data_async().await?;
            let provider = find_provider(&data, &id)?;
            let gateway = find_provider(&data, &gateway_id)?;
            if gateway.identity.protocol != ProviderProtocol::CliProxyApi {
                return Err("请选择 CPA 网关".into());
            }
            let mut file = oauth_accounts::interchange::export(&provider, OAuthExportFormat::Cpa)?;
            // Unique name also prevents two installations racing on the same file.
            file.name = format!(
                "{}-{}.json",
                file.name.trim_end_matches(".json"),
                &oauth_accounts::random_id()?[..12]
            );
            // Pause before sending: a disconnected upload can have succeeded remotely.
            let provider = service.pause_oauth_under_lock(id).await?;
            cli_proxy_api::upload(&data.settings, &gateway, &file)
                .await
                .map_err(|error| {
                    format!("{error}。本机续期已暂停，请先核对 CPA 授权列表再决定是否重试。")
                })?;
            Ok(OAuthExportResult {
                provider: provider.into(),
                file_name: file.name,
                instructions: "授权已发送到 CPA，本机续期已暂停，由 CPA 管理该授权的续期。".into(),
                renewal_paused: true,
                warning: None,
            })
        })
        .await
        .map_err(|_| "CPA 传输任务异常，请核对远端授权列表".to_string())?
    }

    pub(crate) async fn set_cpa_account_disabled(
        &self,
        id: String,
        name: String,
        auth_index: String,
        disabled: bool,
        revision: u64,
    ) -> Result<(), String> {
        let (settings, gateway, _guard) = self.cpa_mutation(&id, &name, revision).await?;
        cli_proxy_api::set_disabled(&settings, &gateway, &name, &auth_index, disabled).await
    }

    pub(crate) async fn set_cpa_account_fields(
        &self,
        id: String,
        name: String,
        auth_index: String,
        fields: CpaCredentialFields,
        revision: u64,
    ) -> Result<(), String> {
        let (settings, gateway, _guard) = self.cpa_mutation(&id, &name, revision).await?;
        cli_proxy_api::patch_fields(&settings, &gateway, &name, &auth_index, &fields).await
    }

    pub(crate) async fn refresh_cpa_account(
        &self,
        id: String,
        name: String,
        auth_index: String,
        revision: u64,
    ) -> Result<(), String> {
        let (settings, gateway, _guard) = self.cpa_mutation(&id, &name, revision).await?;
        cli_proxy_api::refresh_remote(&settings, &gateway, &name, &auth_index).await
    }

    /// Clears a credential's cooldown so CPA may schedule it again. The account
    /// stays locked while the request is in flight, like the other mutations.
    pub(crate) async fn reset_cpa_account_cooldown(
        &self,
        id: String,
        name: String,
        auth_index: String,
        revision: u64,
    ) -> Result<(), String> {
        let (settings, gateway, _guard) = self.cpa_mutation(&id, &name, revision).await?;
        cli_proxy_api::reset_cooldown(&settings, &gateway, &name, &auth_index).await
    }

    /// Refreshes every remote credential. No per-account lock is taken because
    /// the request is not addressed to one account, but the connection still
    /// has to be the one the panel was rendered from.
    pub(crate) async fn refresh_all_cpa_accounts(
        &self,
        id: String,
        revision: u64,
    ) -> Result<(), String> {
        let data = self.load_app_data_async().await?;
        let gateway = find_provider(&data, &id)?;
        if gateway.auth.credential_revision != revision {
            return Err("CPA 连接已变更，请刷新后重试".into());
        }
        cli_proxy_api::refresh_all(&data.settings, &gateway).await
    }

    pub(crate) async fn cpa_error_logs(&self, id: String) -> Result<Vec<CpaErrorLog>, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        cli_proxy_api::error_logs(&data.settings, &provider).await
    }

    /// Scheduling settings belong to the gateway and change outside this app,
    /// so they are read when the panel asks for them, not on every refresh.
    pub(crate) async fn cpa_gateway_settings(
        &self,
        id: String,
    ) -> Result<(CpaGatewaySettings, String), String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::gateway_settings(&client, &provider).await
    }

    /// Gateway scheduling decides where every future request goes, so the
    /// connection still has to be the one the panel was rendered from.
    pub(crate) async fn update_cpa_gateway_settings(
        &self,
        id: String,
        update: CpaGatewaySettingsUpdate,
        revision: u64,
    ) -> Result<(), String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        check_revision(&provider, revision)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::update_gateway_settings(&client, &provider, &update).await
    }

    /// Read-only view of the plugins the gateway reports.
    pub(crate) async fn cpa_plugins(&self, id: String) -> Result<CpaPluginList, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        cli_proxy_api::plugins(&data.settings, &provider).await
    }

    /// What the gateway's store sources offer. Reading it makes the gateway
    /// call the sources, so it only runs when the panel opens the store.
    pub(crate) async fn cpa_plugin_store(&self, id: String) -> Result<CpaPluginStore, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        cli_proxy_api::plugin_store::store(&data.settings, &provider).await
    }

    /// Installs one store entry on the gateway, which downloads and enables it.
    pub(crate) async fn install_cpa_plugin(
        &self,
        id: String,
        plugin_id: String,
        source_id: String,
        version: String,
        revision: u64,
    ) -> Result<CpaStoreInstall, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        check_revision(&provider, revision)?;
        cli_proxy_api::plugin_store::install(
            &data.settings,
            &provider,
            &plugin_id,
            &source_id,
            &version,
        )
        .await
    }

    /// Asks the gateway which release it considers current. The gateway calls
    /// GitHub itself, so this can fail while the rest of the card works.
    pub(crate) async fn check_cpa_version(&self, id: String) -> Result<CpaVersionCheck, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        let current = provider
            .capabilities
            .gateway
            .as_ref()
            .map(|gateway| gateway.gateway_version.clone())
            .unwrap_or_default();
        if current.is_empty() {
            return Err("网关未报告自身版本，请先刷新卡片".into());
        }
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::latest_release(&client, &provider, &current).await
    }

    /// The JSON object the gateway keeps for one plugin. Editing it is how a
    /// plugin gets its credentials and options without a shell on the host.
    pub(crate) async fn cpa_plugin_config(
        &self,
        id: String,
        plugin_id: String,
    ) -> Result<CpaPluginConfig, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::plugin_config(&client, &provider, &plugin_id).await
    }

    pub(crate) async fn set_cpa_plugin_enabled(
        &self,
        id: String,
        plugin_id: String,
        enabled: bool,
        revision: u64,
    ) -> Result<(), String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        check_revision(&provider, revision)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::set_plugin_enabled(&client, &provider, &plugin_id, enabled).await
    }

    pub(crate) async fn update_cpa_plugin_config(
        &self,
        id: String,
        plugin_id: String,
        content: String,
        revision: u64,
    ) -> Result<CpaPluginConfig, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        check_revision(&provider, revision)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::update_plugin_config(&client, &provider, &plugin_id, &content).await
    }

    /// Removes a plugin from the gateway together with its saved configuration.
    pub(crate) async fn remove_cpa_plugin(
        &self,
        id: String,
        plugin_id: String,
        revision: u64,
    ) -> Result<CpaPluginRemoval, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        check_revision(&provider, revision)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::remove_plugin(&client, &provider, &plugin_id).await
    }

    /// The authorization services this gateway can sign in to. The gateway
    /// holds the pending session, so nothing here is stored in this app.
    pub(crate) async fn cpa_login_providers(
        &self,
        id: String,
    ) -> Result<Vec<CpaLoginProvider>, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        cli_proxy_api::login::login_providers(&data.settings, &provider).await
    }

    pub(crate) async fn start_cpa_login(
        &self,
        id: String,
        gateway_provider: String,
    ) -> Result<CpaLoginStart, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::login::start_login(&client, &provider, &gateway_provider).await
    }

    pub(crate) async fn cpa_login_status(
        &self,
        id: String,
        state: String,
        api_version: String,
    ) -> Result<CpaLoginStatus, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::login::login_status(&client, &provider, &state, &api_version).await
    }

    pub(crate) async fn submit_cpa_login_callback(
        &self,
        id: String,
        gateway_provider: String,
        state: String,
        api_version: String,
        input: String,
    ) -> Result<(), String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::login::submit_login_callback(
            &client,
            &provider,
            &gateway_provider,
            &state,
            &api_version,
            &input,
        )
        .await
    }

    pub(crate) async fn cancel_cpa_login(&self, id: String, state: String) -> Result<(), String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::login::cancel_login(&client, &provider, &state).await
    }

    /// One credential's quota as a gateway plugin reads it. The request makes
    /// the gateway ask the provider, so it only runs when the panel asks.
    pub(crate) async fn cpa_account_quota(
        &self,
        id: String,
        auth_index: String,
        plugin_id: String,
        api_version: String,
    ) -> Result<CpaAccountQuota, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::quota::account_quota(
            &client,
            &provider,
            &auth_index,
            &plugin_id,
            &api_version,
        )
        .await
    }

    /// Gateway-wide model rules: what each channel hides from clients and how
    /// it renames models. Read on demand, not with every card refresh.
    pub(crate) async fn cpa_model_rules(&self, id: String) -> Result<CpaModelRules, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::model_rules(&client, &provider).await
    }

    /// The gateway's own model catalog for one OAuth channel. The rules editor
    /// offers these IDs as choices instead of asking the user to type them.
    pub(crate) async fn cpa_model_definitions(
        &self,
        id: String,
        channel: String,
    ) -> Result<Vec<CpaModelDefinition>, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::model_definitions(&client, &provider, &channel).await
    }

    pub(crate) async fn update_cpa_model_rules(
        &self,
        id: String,
        kind: CpaModelRuleKind,
        content: String,
        revision: u64,
    ) -> Result<CpaModelRules, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        check_revision(&provider, revision)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::update_model_rules(&client, &provider, kind, &content).await
    }

    /// Keys downstream callers present to the gateway. They are independent of
    /// the management key, so they are read only when the panel asks.
    pub(crate) async fn cpa_client_keys(&self, id: String) -> Result<CpaClientKeys, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        cli_proxy_api::client_keys(&client, &provider).await
    }

    /// Adds one client key to the list the gateway already serves. An empty
    /// value asks this app to mint one, which the panel then shows in full so
    /// the user can copy it before it is masked again.
    pub(crate) async fn add_cpa_client_key(
        &self,
        id: String,
        input: CpaClientKeyAdd,
        revision: u64,
    ) -> Result<CpaClientKeyAdded, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        check_revision(&provider, revision)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        let mut added = String::new();
        let list = cli_proxy_api::update_client_keys(&client, &provider, |keys| {
            let value = normalize_client_key(&input.value, keys)?;
            keys.push(value.clone());
            added = value;
            Ok(())
        })
        .await?;
        Ok(CpaClientKeyAdded { list, value: added })
    }

    /// Removes one client key. CPA replaces the list wholesale, so the key is
    /// looked up in what the gateway serves right now instead of trusting an
    /// index the panel rendered earlier.
    pub(crate) async fn remove_cpa_client_key(
        &self,
        id: String,
        value: String,
        revision: u64,
    ) -> Result<CpaClientKeys, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        check_revision(&provider, revision)?;
        let client = crate::adapters::oauth::http::client(&data.settings, &provider).await?;
        let value = value.trim().to_owned();
        cli_proxy_api::update_client_keys(&client, &provider, |keys| {
            if !keys.contains(&value) {
                return Err("该客户端密钥已不在网关上，请刷新后重试".into());
            }
            keys.retain(|key| *key != value);
            Ok(())
        })
        .await
    }

    /// Tail of the gateway's own log file. A non-empty cursor continues from
    /// the previous read instead of re-reading the tail.
    pub(crate) async fn cpa_request_logs(
        &self,
        id: String,
        limit: u32,
        cursor: String,
    ) -> Result<CpaLogTail, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        cli_proxy_api::request_logs(&data.settings, &provider, limit, &cursor).await
    }

    /// The API-key credentials in the gateway's configuration are not auth
    /// files, so their request counts come from the usage endpoint instead.
    pub(crate) async fn cpa_api_key_usage(
        &self,
        id: String,
    ) -> Result<Vec<CpaApiKeyUsage>, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        cli_proxy_api::api_key_usage(&data.settings, &provider).await
    }

    /// The shared log interleaves every request; CPA keeps a separate file per
    /// request, which this reads so a drill-down shows one request's own lines.
    pub(crate) async fn cpa_request_log(
        &self,
        id: String,
        request_id: String,
    ) -> Result<CpaRequestLog, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        cli_proxy_api::request_log(&data.settings, &provider, &request_id).await
    }

    /// Clearing deletes the gateway's log files, so the connection still has to
    /// be the one the panel was rendered from.
    pub(crate) async fn clear_cpa_logs(
        &self,
        id: String,
        revision: u64,
    ) -> Result<CpaLogClear, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        check_revision(&provider, revision)?;
        cli_proxy_api::clear_logs(&data.settings, &provider).await
    }

    /// The gateway's own configuration document. It carries values this panel
    /// does not model, so it is offered as-is rather than through fields.
    pub(crate) async fn cpa_gateway_config(&self, id: String) -> Result<CpaGatewayConfig, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        cli_proxy_api::gateway_config(&data.settings, &provider).await
    }

    /// A save replaces the gateway's whole configuration, so the connection
    /// still has to be the one the text was read from.
    pub(crate) async fn update_cpa_gateway_config(
        &self,
        id: String,
        content: String,
        etag: String,
        api_version: String,
        revision: u64,
    ) -> Result<(), String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        check_revision(&provider, revision)?;
        cli_proxy_api::update_gateway_config(
            &data.settings,
            &provider,
            &content,
            &etag,
            &api_version,
        )
        .await
    }

    /// Whether the gateway writes the log this panel tails. Read when the log
    /// is opened, because a gateway that is not logging explains the empty
    /// view better than an error does.
    pub(crate) async fn cpa_log_settings(&self, id: String) -> Result<CpaLogSettings, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        cli_proxy_api::log_settings(&data.settings, &provider).await
    }

    /// The switch writes the gateway's config, so the connection still has to
    /// be the one the panel was rendered from.
    pub(crate) async fn set_cpa_log_settings(
        &self,
        id: String,
        enabled: bool,
        revision: u64,
    ) -> Result<(), String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        check_revision(&provider, revision)?;
        cli_proxy_api::set_log_settings(&data.settings, &provider, enabled).await
    }

    pub(crate) async fn read_cpa_error_log(
        &self,
        id: String,
        name: String,
    ) -> Result<CpaErrorLogContent, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        cli_proxy_api::fetch_error_log(&data.settings, &provider, &name).await
    }

    /// Removing a credential is irreversible on the CPA side, so it holds the
    /// same per-account lock and revision check as the other mutations.
    pub(crate) async fn delete_cpa_account(
        &self,
        id: String,
        name: String,
        auth_index: String,
        revision: u64,
    ) -> Result<(), String> {
        let (settings, gateway, _guard) = self.cpa_mutation(&id, &name, revision).await?;
        cli_proxy_api::delete(&settings, &gateway, &name, &auth_index).await
    }

    pub(crate) async fn read_cpa_account_file(
        &self,
        id: String,
        name: String,
    ) -> Result<CpaCredentialFile, String> {
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, &id)?;
        cli_proxy_api::fetch_file(&data.settings, &provider, &name).await
    }

    /// Serializes remote writes per account and rejects a connection whose
    /// credentials changed after the panel was rendered. The returned guard
    /// must stay alive for the whole mutation.
    async fn cpa_mutation(
        &self,
        id: &str,
        name: &str,
        revision: u64,
    ) -> Result<(AppSettings, Provider, tokio::sync::OwnedMutexGuard<()>), String> {
        let guard = oauth_accounts::account_lock(&format!("cpa:{id}:{name}"))
            .lock_owned()
            .await;
        let data = self.load_app_data_async().await?;
        let provider = find_provider(&data, id)?;
        if provider.auth.credential_revision != revision {
            return Err("CPA 连接已变更，请刷新后重试".into());
        }
        Ok((data.settings, provider, guard))
    }
}

/// A mutation must be built on the connection the panel rendered. The
/// revision changes whenever the management key or address is replaced.
fn check_revision(provider: &Provider, revision: u64) -> Result<(), String> {
    if provider.auth.credential_revision != revision {
        return Err("CPA 连接已变更，请刷新后重试".into());
    }
    Ok(())
}

/// Validates a client key the panel asked to add, or mints one when the panel
/// left the field empty. The gateway accepts any string, so the rules kept
/// here are the ones that keep the list usable: one line, no duplicates, and a
/// bounded length.
fn normalize_client_key(value: &str, existing: &[String]) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(format!("sk-{}", &oauth_accounts::random_id()?[..48]));
    }
    if value.chars().count() > 256 {
        return Err("客户端密钥过长".into());
    }
    if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
        return Err("客户端密钥不能包含空白或控制字符".into());
    }
    if existing.iter().any(|key| key == value) {
        return Err("该客户端密钥已在网关上".into());
    }
    Ok(value.to_owned())
}

fn save_authorization_file(path: &str, content: &str) -> Result<(), String> {
    let path = Path::new(path);
    if !path.is_absolute() || path.extension().is_none_or(|extension| extension != "json") {
        return Err("请选择 JSON 文件保存位置".into());
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            "该文件已存在，请选择新文件名，避免覆盖其他授权"
        } else {
            "无法创建授权文件，请检查目录权限"
        }
    })?;
    if file
        .write_all(content.as_bytes())
        .and_then(|_| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(path);
        return Err("授权文件写入失败，本机续期状态未改变".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalize_client_key;

    #[test]
    fn a_client_key_is_trimmed_and_must_stay_unique() {
        let existing = vec!["sk-existing".to_string()];
        assert_eq!(
            normalize_client_key("  sk-new  ", &existing).unwrap(),
            "sk-new"
        );
        assert_eq!(
            normalize_client_key("sk-existing", &existing).unwrap_err(),
            "该客户端密钥已在网关上"
        );
        assert_eq!(
            normalize_client_key("sk-two words", &existing).unwrap_err(),
            "客户端密钥不能包含空白或控制字符"
        );
        assert_eq!(
            normalize_client_key(&"k".repeat(257), &existing).unwrap_err(),
            "客户端密钥过长"
        );
    }

    #[test]
    fn an_empty_client_key_is_minted_once_and_stays_unique() {
        let first = normalize_client_key("", &[]).unwrap();
        let second = normalize_client_key("", &[]).unwrap();
        assert!(first.starts_with("sk-"));
        assert_eq!(first.len(), 51);
        assert_ne!(first, second);
    }
}
