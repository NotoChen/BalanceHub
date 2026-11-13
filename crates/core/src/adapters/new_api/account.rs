use crate::models::Provider;
use reqwest::Method;
use serde_json::json;

use super::http::{
    build_url, build_user_request, provider_user_management_context, ProviderTransport,
};
use super::response::{parse_success_data, send_text};

pub async fn update_user_profile(
    client: &ProviderTransport,
    provider: &Provider,
    username: &str,
    display_name: &str,
) -> Result<String, String> {
    let username = username.trim();
    if username.is_empty() {
        return Err("用户名不能为空".to_string());
    }
    let (base_url, api_user, credential) = provider_user_management_context(provider)?;
    let url = build_url(&base_url, "/api/user/self")?;
    let request = build_user_request(client, Method::PUT, url, &base_url, &api_user, credential)
        .json(&json!({
            "username": username,
            "display_name": display_name.trim(),
        }));

    let (status, body) = send_text(client, request, "修改账号资料").await?;
    parse_success_data(&status, body, "修改账号资料")?;
    Ok("账号资料已更新".to_string())
}

pub async fn redeem_code(
    client: &ProviderTransport,
    provider: &Provider,
    code: &str,
) -> Result<String, String> {
    let code = code.trim();
    if code.is_empty() {
        return Err("请输入兑换码".to_string());
    }
    let (base_url, api_user, credential) = provider_user_management_context(provider)?;
    let url = build_url(&base_url, "/api/user/topup")?;
    let request = build_user_request(client, Method::POST, url, &base_url, &api_user, credential)
        .json(&json!({ "key": code }));

    let (status, body) = send_text(client, request, "兑换码兑换").await?;
    parse_success_data(&status, body, "兑换码兑换")?;
    Ok("兑换成功，余额已更新".to_string())
}

pub async fn change_user_password(
    client: &ProviderTransport,
    provider: &Provider,
    original_password: &str,
    password: &str,
) -> Result<String, String> {
    let original_password = original_password.trim();
    let password = password.trim();
    if password.is_empty() {
        return Err("请输入新密码".to_string());
    }

    let (base_url, api_user, credential) = provider_user_management_context(provider)?;
    let url = build_url(&base_url, "/api/user/self")?;
    let request = build_user_request(client, Method::PUT, url, &base_url, &api_user, credential)
        .json(&json!({
            "original_password": original_password,
            "password": password,
        }));

    let (status, body) = send_text(client, request, "修改用户密码").await?;
    parse_success_data(&status, body, "修改用户密码")?;
    Ok("密码已更新".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::test_support::{serve, Reply};
    use crate::models::AppSettings;

    async fn client(provider: &Provider) -> crate::adapters::transport::ProviderTransport {
        crate::adapters::transport::build_client(&AppSettings::default(), provider)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn profile_update_sends_only_editable_profile_fields() {
        let (provider, requests, server) = serve(vec![Reply(200, r#"{"success":true,"data":{}}"#)]);
        let client = client(&provider).await;
        let message = update_user_profile(&client, &provider, " alice ", " Alice ")
            .await
            .unwrap();
        server.join().unwrap();
        let request = requests.recv().unwrap();
        assert!(request.starts_with("PUT /relay/api/user/self "));
        assert!(request.contains("\"username\":\"alice\""));
        assert!(request.contains("\"display_name\":\"Alice\""));
        assert!(!request.contains("password"));
        assert_eq!(message, "账号资料已更新");
    }

    #[tokio::test]
    async fn profile_update_rejects_an_empty_username_before_sending() {
        let (provider, _requests, _server) = serve(Vec::new());
        let client = client(&provider).await;
        let error = update_user_profile(&client, &provider, "  ", "Alice")
            .await
            .unwrap_err();
        assert!(error.contains("用户名不能为空"));
    }

    #[tokio::test]
    async fn redemption_posts_the_code_and_surfaces_the_site_message() {
        let (provider, requests, server) = serve(vec![Reply(
            200,
            r#"{"success":true,"data":500000,"message":"兑换成功"}"#,
        )]);
        let client = client(&provider).await;
        let message = redeem_code(&client, &provider, " CODE-42 ").await.unwrap();
        server.join().unwrap();
        let request = requests.recv().unwrap();
        assert!(request.starts_with("POST /relay/api/user/topup "));
        assert!(request.contains("\"key\":\"CODE-42\""));
        assert!(message.contains("余额已更新"));
    }

    #[tokio::test]
    async fn redemption_rejects_an_empty_code_before_sending() {
        let (provider, _requests, _server) = serve(Vec::new());
        let client = client(&provider).await;
        let error = redeem_code(&client, &provider, " ").await.unwrap_err();
        assert!(error.contains("请输入兑换码"));
    }
}
