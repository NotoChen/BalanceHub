use crate::{
    adapters::protocol::{contracts::ProviderCredentialPatch, ProtocolAdapter},
    models::{
        AppSettings, AuthMode, Provider, ProviderCredentialCompletionResult, ProviderInput,
        ProviderProtocol,
    },
    state::AppState,
    util::unix_millis as current_timestamp_millis,
};
use tauri::Manager;

use super::{MutationDecision, ProviderRequestContext, ProviderService};

pub(super) static AUTH_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

impl<'a> ProviderService<'a> {
    /// Authentication survives cancellation of the caller. The dedicated gate
    /// remains owned until a rotated cookie has been persisted, even if the UI
    /// drops its wait and releases the broader business-operation gate.
    pub(super) async fn prepare_operation_auth(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(Provider, Result<(), String>), String> {
        if provider.identity.protocol != ProviderProtocol::NewApi
            || !matches!(provider.auth.mode, AuthMode::Session | AuthMode::Password)
            || (provider.auth.mode != AuthMode::Password
                && provider.auth.new_api_session.is_none()
                && !provider
                    .auth
                    .session_cookie
                    .split(';')
                    .any(|part| part.trim().starts_with("new_api_refresh=")))
        {
            return Ok((provider.clone(), Ok(())));
        }
        let app = self.app.clone();
        let settings = settings.clone();
        let provider = provider.clone();
        tauri::async_runtime::spawn(async move {
            let _gate = AUTH_GATE.lock().await;
            let service = ProviderService::new(&app);
            let context = ProviderRequestContext::capture(&provider);
            service
                .current_operation_provider(&context)
                .await?
                .ok_or("账号配置已变更，请重试")?;
            let operation =
                crate::adapters::new_api::prepare_authentication(&settings, &provider).await?;
            let stored = service
                .persist_operation_credentials(&context, &operation.credentials)
                .await?
                .ok_or("账号配置已变更，登录结果未写入当前账号")?;
            Ok((stored, operation.value))
        })
        .await
        .map_err(|_| "登录会话更新任务异常".to_string())?
    }

    pub(super) async fn prepare_operation_provider(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<Provider, String> {
        let (provider, result) = self.prepare_operation_auth(settings, provider).await?;
        result?;
        Ok(provider)
    }

    /// Resolve backend-owned sessions from storage, never from an old editor
    /// snapshot. A pasted rotating cookie requires saving or browser import.
    pub(super) async fn prepare_input_provider(
        &self,
        settings: &AppSettings,
        input: ProviderInput,
    ) -> Result<Provider, String> {
        let id = input
            .id
            .clone()
            .unwrap_or_else(|| format!("provider-{}", current_timestamp_millis()));
        let mut candidate = Provider::from_input(input.clone(), id.clone());
        if let Some(stored) = self
            .snapshot_async()
            .await?
            .providers
            .iter()
            .find(|provider| provider.identity.id == id)
        {
            let mut applied = stored.clone();
            applied.apply_input(input);
            if ProviderRequestContext::capture(stored).matches(&applied) {
                candidate = self.prepare_operation_provider(settings, stored).await?;
                candidate.proxy = applied.proxy;
                return Ok(candidate);
            }
            candidate.auth.new_api_session = None;
        }
        if candidate.auth.new_api_session.is_some()
            || candidate
                .auth
                .session_cookie
                .split(';')
                .any(|part| part.trim().starts_with("new_api_refresh="))
        {
            return Err("请先保存中转站，或使用登录并导入来接管可续期会话".to_string());
        }
        Ok(candidate)
    }

    pub async fn complete_credentials(
        &self,
        input: ProviderInput,
    ) -> Result<ProviderCredentialCompletionResult, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = self
            .prepare_input_provider(&data.settings, input.clone())
            .await?;
        let mut input = input;
        input.auth = provider.auth;
        ProtocolAdapter
            .complete_credentials(&data.settings, input, provider.identity.id)
            .await
    }

    pub async fn generate_access_token_for_input(
        &self,
        input: ProviderInput,
    ) -> Result<String, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = self.prepare_input_provider(&data.settings, input).await?;
        ProtocolAdapter
            .generate_access_token(&data.settings, &provider)
            .await
    }

    /// Persist credentials produced by an authenticated adapter operation.
    ///
    /// Authentication can refresh a JWT, rotate a refresh token, or turn a
    /// password login into a reusable NewAPI session. The operation started
    /// from a snapshot, so merge only when that exact snapshot is still active.
    pub(super) async fn current_operation_provider(
        &self,
        request_context: &ProviderRequestContext,
    ) -> Result<Option<Provider>, String> {
        let app = self.app.clone();
        let request_context = request_context.clone();
        tauri::async_runtime::spawn_blocking(move || {
            app.state::<AppState>()
                .data
                .read()
                .unwrap_or_else(|err| err.into_inner())
                .providers
                .iter()
                .find(|stored| request_context.matches(stored))
                .cloned()
        })
        .await
        .map_err(|err| format!("读取认证快照任务异常: {err}"))
    }

    pub(super) async fn persist_operation_credentials(
        &self,
        request_context: &ProviderRequestContext,
        credentials: &ProviderCredentialPatch,
    ) -> Result<Option<Provider>, String> {
        if credentials.is_empty() {
            return self.current_operation_provider(request_context).await;
        }

        let request_context = request_context.clone();
        let credentials = credentials.clone();
        self.mutate_decided_async(move |data| {
            let Some(stored) = data
                .providers
                .iter_mut()
                .find(|stored| request_context.matches(stored))
            else {
                return Ok(MutationDecision::unchanged(None));
            };
            let changed = credentials.apply(stored);
            let provider = Some(stored.clone());
            Ok(if changed {
                MutationDecision::changed(provider)
            } else {
                MutationDecision::unchanged(provider)
            })
        })
        .await
    }
}
