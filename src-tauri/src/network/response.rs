use crate::limits;
use reqwest::Response;

pub(crate) async fn read_http_text(response: Response, context: &str) -> Result<String, String> {
    read_text_limited(response, limits::MAX_HTTP_RESPONSE_BYTES, context).await
}

pub(crate) async fn read_webhook_text(response: Response, context: &str) -> Result<String, String> {
    read_text_limited(response, limits::MAX_WEBHOOK_RESPONSE_BYTES, context).await
}

async fn read_text_limited(
    mut response: Response,
    max_bytes: usize,
    context: &str,
) -> Result<String, String> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(limit_error(context, max_bytes));
    }

    let mut bytes = Vec::with_capacity(
        response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or_default()
            .min(max_bytes),
    );
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("{context}响应读取失败: {err}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(limit_error(context, max_bytes));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn limit_error(context: &str, max_bytes: usize) -> String {
    format!(
        "{context}响应超过 {} MiB 安全上限，已停止读取",
        max_bytes / 1024 / 1024
    )
}
