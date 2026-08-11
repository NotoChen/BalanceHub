use super::Sub2ApiAdapter;
use crate::{
    adapters::{
        sub2_api::{
            auth::request_account_json,
            json::string_field,
            keys::{api_key_from_value, fetch_api_keys},
            response::normalize_base_url,
            usage::urlencoding,
        },
        transport::{build_client, ProviderTransport},
    },
    models::{AppSettings, AuthMode, Provider, ProviderApiKeyOption, ProviderCapabilities},
};
use reqwest::Method;
use serde_json::json;

impl Sub2ApiAdapter {
    pub(crate) async fn list_api_keys(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(Provider, Vec<ProviderApiKeyOption>), String> {
        let client = build_client(settings, provider).await?;
        let (authenticated, options) = fetch_api_keys(&client, provider).await?;
        Ok((authenticated, options))
    }

    pub(crate) async fn create_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        name: &str,
    ) -> Result<(Provider, ProviderApiKeyOption), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("请填写 API 密钥名称".to_string());
        }
        let client = build_client(settings, provider).await?;
        let (authenticated, data) = request_account_json(
            &client,
            provider,
            Method::POST,
            "/keys",
            Some(json!({"name": name})),
            "创建 API Key",
        )
        .await?;
        let option = api_key_from_value(&data)
            .ok_or_else(|| "创建成功但响应中没有返回 API Key".to_string())?;
        Ok((authenticated, option))
    }

    pub(crate) async fn generate_access_token(
        &self,
        _settings: &AppSettings,
        provider: &Provider,
    ) -> Result<String, String> {
        if !provider.auth.access_token.trim().is_empty() {
            return Ok(provider.auth.access_token.clone());
        }
        Err("Sub2API 的访问令牌由账号密码登录自动生成，不能通过 API Key 创建".to_string())
    }

    pub(crate) async fn delete_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        token_id: &str,
    ) -> Result<(Provider, ()), String> {
        let token_id = token_id.trim();
        if token_id.is_empty() {
            return Err("缺少 API Key ID".to_string());
        }
        let client = build_client(settings, provider).await?;
        let (authenticated, _) = request_account_json(
            &client,
            provider,
            Method::DELETE,
            &format!("/keys/{token_id}"),
            None,
            "删除 API Key",
        )
        .await?;
        Ok((authenticated, ()))
    }

    pub(crate) async fn change_password(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        original_password: &str,
        password: &str,
    ) -> Result<(Provider, String), String> {
        if password.trim().is_empty() {
            return Err("请输入新密码".to_string());
        }
        let client = build_client(settings, provider).await?;
        let (authenticated, _) = request_account_json(
            &client,
            provider,
            Method::PUT,
            "/user/password",
            Some(json!({
                "old_password": original_password,
                "new_password": password.trim(),
            })),
            "修改 Sub2API 密码",
        )
        .await?;
        Ok((authenticated, "密码已更新".to_string()))
    }

    pub(crate) async fn probe_capabilities(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(Provider, (ProviderCapabilities, String, Option<String>)), String> {
        let mut capabilities = ProviderCapabilities {
            check_in_known: true,
            check_in_supported: false,
            api_key_management_known: true,
            api_key_management_supported: false,
            invitation_known: true,
            invitation_supported: false,
            ..Default::default()
        };
        if matches!(provider.auth.mode, AuthMode::ApiKey) {
            return Ok((provider.clone(), (capabilities, String::new(), None)));
        }
        let client = build_client(settings, provider).await?;
        let mut errors = Vec::new();
        let mut authenticated = provider.clone();
        match fetch_api_keys(&client, provider).await {
            Ok((next, _)) => {
                authenticated = next;
                capabilities.api_key_management_supported = true;
            }
            Err(message) => errors.push(format!("密钥管理: {message}")),
        }
        let invite = match self.invite_link_with_client(&client, &authenticated).await {
            Ok((next, link)) => {
                authenticated = next;
                capabilities.invitation_supported = true;
                link
            }
            Err(message) => {
                errors.push(format!("邀请链接: {message}"));
                String::new()
            }
        };
        Ok((
            authenticated,
            (
                capabilities,
                invite,
                (!errors.is_empty()).then(|| errors.join("；")),
            ),
        ))
    }

    pub(crate) async fn invite_link(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(Provider, String), String> {
        let client = build_client(settings, provider).await?;
        self.invite_link_with_client(&client, provider).await
    }

    async fn invite_link_with_client(
        &self,
        client: &ProviderTransport,
        provider: &Provider,
    ) -> Result<(Provider, String), String> {
        let (authenticated, data) = request_account_json(
            client,
            provider,
            Method::GET,
            "/user/aff",
            None,
            "读取邀请链接",
        )
        .await?;
        let code = string_field(&data, &["aff_code", "affCode", "code"])
            .ok_or_else(|| "Sub2API 没有返回邀请编码".to_string())?;
        Ok((
            authenticated,
            format!(
                "{}/register?aff_code={}",
                normalize_base_url(&provider.identity.base_url),
                urlencoding(&code)
            ),
        ))
    }
}
