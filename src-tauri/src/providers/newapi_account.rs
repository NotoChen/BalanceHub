use crate::models::Provider;
use reqwest::{Client, Method};
use serde_json::json;

use super::newapi_http::{build_url, build_user_request, provider_user_management_context};
use super::newapi_response::{parse_success_data, send_text};

pub async fn change_user_password(
    client: &Client,
    provider: &Provider,
    original_password: &str,
    password: &str,
) -> Result<String, String> {
    let original_password = original_password.trim();
    let password = password.trim();
    if original_password.is_empty() {
        return Err("请输入原密码".to_string());
    }
    if password.is_empty() {
        return Err("请输入新密码".to_string());
    }

    let (base_url, api_user, credential, is_anyrouter) =
        provider_user_management_context(provider)?;
    let url = build_url(&base_url, "/api/user/self")?;
    let request = build_user_request(
        client,
        Method::PUT,
        url,
        &base_url,
        &api_user,
        credential,
        is_anyrouter,
    )
    .await?
    .json(&json!({
        "original_password": original_password,
        "password": password,
    }));

    let (status, body) = send_text(request, "修改用户密码").await?;
    parse_success_data(&status, body, "修改用户密码")?;
    Ok("密码已更新".to_string())
}
