use crate::{
    adapters::{
        api::parse_model_data, protocol::contracts::ProviderOperationOutcome,
        transport::ProviderTransport,
    },
    models::{
        provider_domain::{auth, model_list},
        Provider, ProviderModelList, ProviderModelScope,
    },
};
use reqwest::Method;

use super::{
    http::authenticate_password_provider,
    http::{
        build_url, build_user_request, provider_user_management_context, retry_with_access_token,
    },
    response::{parse_success_data, send_text},
};

pub(super) async fn fetch_models(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<ProviderOperationOutcome<ProviderModelList>, String> {
    fetch_models_with_account_auth(client, provider, |candidate| {
        std::future::ready(Ok(candidate))
    })
    .await
}

/// The service persists rotating account credentials before the model request.
/// Invoke this hook only when Key discovery actually needs account fallback.
pub(crate) async fn fetch_models_with_account_auth<Prepare, Prepared>(
    client: &ProviderTransport,
    provider: &Provider,
    prepare_account: Prepare,
) -> Result<ProviderOperationOutcome<ProviderModelList>, String>
where
    Prepare: FnOnce(Provider) -> Prepared,
    Prepared: std::future::Future<Output = Result<Provider, String>>,
{
    let key_error = if auth::has_api_key(provider) {
        match crate::adapters::api::fetch_models(client, provider).await {
            // An empty successful response still describes this Key's actual scope.
            Ok(models) => {
                return Ok(ProviderOperationOutcome::unchanged(ProviderModelList::new(
                    models,
                    ProviderModelScope::ApiKey,
                )))
            }
            Err(error) if !model_list::can_use_account_models(provider) => return Err(error),
            Err(error) => Some(error),
        }
    } else {
        None
    };
    let account_result = async {
        let prepared = prepare_account(provider.clone()).await?;
        fetch_account_models(client, &prepared).await
    }
    .await;
    let (authenticated, models) = account_result.map_err(|error| match &key_error {
        Some(key_error) => {
            format!("Key 模型接口失败：{key_error}；账号模型接口也失败：{error}")
        }
        None => error,
    })?;
    Ok(ProviderOperationOutcome::authenticated(
        provider,
        authenticated,
        ProviderModelList {
            models,
            scope: ProviderModelScope::Account,
            fallback_reason: key_error,
        },
    ))
}

pub(super) async fn fetch_account_models(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<(Provider, Vec<String>), String> {
    let authenticated = authenticate_password_provider(client, provider).await?;
    retry_with_access_token(
        client,
        &authenticated,
        fetch_once(client, &authenticated),
        |candidate| {
            let client = client.clone();
            async move { fetch_once(&client, &candidate).await }
        },
    )
    .await
}

async fn fetch_once(
    client: &ProviderTransport,
    provider: &Provider,
) -> Result<Vec<String>, String> {
    let (base_url, api_user, credential) = provider_user_management_context(provider)?;
    let request = build_user_request(
        client,
        Method::GET,
        build_url(&base_url, "/api/user/models")?,
        &base_url,
        &api_user,
        credential,
    );
    let (status, body) = send_text(client, request, "读取账号模型列表").await?;
    let data = parse_success_data(&status, body, "账号模型列表")?;
    parse_model_data(&data)
}

#[cfg(test)]
#[path = "models/tests.rs"]
mod tests;
