use super::Sub2ApiAdapter;
use crate::{
    adapters::{
        sub2_api::{
            auth::request_account_json, json::string_field, keys::fetch_api_keys,
            response::normalize_base_url,
        },
        transport::{build_client, ProviderTransport},
    },
    models::{
        AppSettings, AuthMode, Provider, ProviderApiKeyEditorContext, ProviderApiKeyOption,
        ProviderApiKeyPatch, ProviderCapabilities,
    },
};
use reqwest::Method;
use serde_json::json;

impl Sub2ApiAdapter {
    pub async fn list_api_keys(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(Provider, Vec<ProviderApiKeyOption>), String> {
        let client = build_client(settings, provider).await?;
        let (authenticated, options) = fetch_api_keys(&client, provider).await?;
        Ok((authenticated, options))
    }

    pub async fn api_key_editor_context(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        token_id: Option<&str>,
    ) -> Result<(Provider, ProviderApiKeyEditorContext), String> {
        let client = build_client(settings, provider).await?;
        crate::adapters::sub2_api::key_management::editor_context(&client, provider, token_id).await
    }

    pub async fn create_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        patch: &ProviderApiKeyPatch,
    ) -> Result<(Provider, ProviderApiKeyOption), String> {
        let client = build_client(settings, provider).await?;
        crate::adapters::sub2_api::key_management::create(&client, provider, patch).await
    }

    pub async fn update_api_key(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        token_id: &str,
        patch: &ProviderApiKeyPatch,
    ) -> Result<(Provider, ProviderApiKeyOption), String> {
        let client = build_client(settings, provider).await?;
        crate::adapters::sub2_api::key_management::update(&client, provider, token_id, patch).await
    }

    pub async fn generate_access_token(
        &self,
        _settings: &AppSettings,
        provider: &Provider,
    ) -> Result<String, String> {
        if !provider.auth.access_token.trim().is_empty() {
            return Ok(provider.auth.access_token.clone());
        }
        Err("Sub2API 的访问令牌由账号密码登录自动生成，不能通过 API Key 创建".to_string())
    }

    pub async fn delete_api_key(
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

    pub async fn change_password(
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

    pub async fn redeem_code(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        code: &str,
    ) -> Result<(Provider, String), String> {
        let code = code.trim();
        if code.is_empty() {
            return Err("请输入兑换码".to_string());
        }
        let client = build_client(settings, provider).await?;
        let (authenticated, data) = request_account_json(
            &client,
            provider,
            Method::POST,
            "/redeem",
            Some(json!({ "code": code })),
            "兑换 Sub2API 卡密",
        )
        .await?;
        // Sub2API 的兑换结果带本地化文案和新的余额/并发数；站点给了文案就
        // 原样展示，没有时用统一提示，余额刷新由服务层继续完成。
        let message = string_field(&data, &["message"])
            .filter(|message| !message.trim().is_empty())
            .unwrap_or_else(|| "兑换成功，余额已更新".to_string());
        Ok((authenticated, message))
    }

    pub async fn probe_capabilities(
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
            let client = build_client(settings, provider).await?;
            let pricing = super::super::pricing::probe_pricing(&client, provider).await;
            capabilities.pricing = pricing.snapshot;
            let (_, states) =
                super::super::optional_probe::probe_optional(&client, provider, pricing.state)
                    .await;
            for state in states {
                capabilities.set_optional_state(state);
            }
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
        let pricing = super::super::pricing::probe_pricing(&client, &authenticated).await;
        capabilities.pricing = pricing.snapshot;
        let (authenticated, states) =
            super::super::optional_probe::probe_optional(&client, &authenticated, pricing.state)
                .await;
        for state in states {
            capabilities.set_optional_state(state);
        }
        Ok((
            authenticated,
            (
                capabilities,
                invite,
                (!errors.is_empty()).then(|| errors.join("；")),
            ),
        ))
    }

    pub async fn invite_link(
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
            crate::models::provider_domain::endpoints::provider_link(
                provider.identity.protocol,
                &provider.request.endpoints,
                "register_link",
                &normalize_base_url(&provider.identity.base_url),
                &code,
            )
            .ok_or_else(|| "当前协议不支持邀请链接".to_string())?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::test_support::{serve, Reply};
    use crate::models::ProviderProtocol;

    fn sub2_provider(provider: &mut Provider) {
        provider.identity.protocol = ProviderProtocol::Sub2Api;
        provider.auth.mode = AuthMode::AccessToken;
        provider.auth.access_token = "fixture-jwt".to_string();
    }

    #[tokio::test]
    async fn redemption_posts_the_code_and_returns_the_site_message() {
        let (mut provider, requests, server) = serve(vec![Reply(
            200,
            r#"{"code":0,"data":{"message":"兑换成功","new_balance":12.5}}"#,
        )]);
        sub2_provider(&mut provider);
        let (_, message) = Sub2ApiAdapter
            .redeem_code(&AppSettings::default(), &provider, " CODE-42 ")
            .await
            .unwrap();
        server.join().unwrap();
        let request = requests.recv().unwrap();
        assert!(request.starts_with("POST /relay/api/v1/redeem "));
        assert!(request.contains("\"code\":\"CODE-42\""));
        assert_eq!(message, "兑换成功");
    }

    #[tokio::test]
    async fn redemption_rejects_an_empty_code_before_sending() {
        let (mut provider, _requests, _server) = serve(Vec::new());
        sub2_provider(&mut provider);
        let error = Sub2ApiAdapter
            .redeem_code(&AppSettings::default(), &provider, " ")
            .await
            .unwrap_err();
        assert!(error.contains("请输入兑换码"));
    }

    #[tokio::test]
    async fn redemption_surfaces_the_site_error_message() {
        let (mut provider, _requests, server) =
            serve(vec![Reply(200, r#"{"code":400,"message":"卡密已被使用"}"#)]);
        sub2_provider(&mut provider);
        let error = Sub2ApiAdapter
            .redeem_code(&AppSettings::default(), &provider, "USED")
            .await
            .unwrap_err();
        server.join().unwrap();
        assert!(error.contains("卡密已被使用"));
    }
}
