use crate::adapters::transport::ProviderTransport;
use reqwest::StatusCode;
use serde_json::Value;

/// NewAPI 各接口共用的发送器。明确命中盾页时由 ProviderTransport 求解并有限重放；
/// 未命中挑战时仍只发送一次普通 HTTP 请求。
pub(crate) async fn send_text(
    client: &ProviderTransport,
    request: reqwest::RequestBuilder,
    context: &str,
) -> Result<(StatusCode, String), String> {
    let response = client.send(request, context).await?;
    Ok((response.status, response.body))
}

pub(crate) fn parse_success_data(
    status: &StatusCode,
    body: String,
    context: &str,
) -> Result<Value, String> {
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status.as_u16(), trim_message(&body)));
    }

    let decoded = serde_json::from_str::<Value>(&body)
        .map_err(|err| format!("解析{context}响应失败: {err}: {}", trim_message(&body)))?;

    // Most NewAPI endpoints use `success`; the token-usage endpoint follows
    // its OpenAI-compatible response shape and uses boolean `code` instead.
    // Accept only an explicit boolean from either contract so arbitrary JSON
    // cannot be mistaken for a successful business response.
    let success = decoded
        .get("success")
        .and_then(Value::as_bool)
        .or_else(|| decoded.get("code").and_then(Value::as_bool))
        .ok_or_else(|| {
            format!(
                "{context}响应缺少 success/code 字段: {}",
                trim_message(&body)
            )
        })?;
    if !success {
        let message = decoded
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("接口返回失败");
        return Err(message.to_string());
    }

    Ok(decoded.get("data").cloned().unwrap_or(decoded))
}

pub(crate) fn extract_token_items(data: &Value) -> Vec<Value> {
    if let Some(items) = data.get("items").and_then(Value::as_array) {
        return items.clone();
    }

    if let Some(items) = data.as_array() {
        return items.clone();
    }

    if let Some(object) = data.as_object() {
        return object
            .values()
            .filter(|value| value.as_object().is_some())
            .cloned()
            .collect();
    }

    Vec::new()
}

pub(crate) fn extract_usage_items(data: &Value) -> Vec<Value> {
    if let Some(items) = data.get("items").and_then(Value::as_array) {
        return items.clone();
    }

    if let Some(items) = data.as_array() {
        return items.clone();
    }

    Vec::new()
}

pub(crate) fn extract_string_field(value: &Value, field_names: &[&str]) -> Option<String> {
    field_names.iter().find_map(|field_name| {
        let item = value.get(*field_name)?;
        if let Some(text) = item.as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        if item.is_number() {
            return Some(item.to_string());
        }
        None
    })
}

pub(crate) fn extract_i64_field(value: &Value, field_names: &[&str]) -> Option<i64> {
    field_names.iter().find_map(|field_name| {
        value.get(*field_name).and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
                .or_else(|| value.as_f64().map(|number| number as i64))
                .or_else(|| {
                    value
                        .as_str()
                        .and_then(|text| text.trim().parse::<i64>().ok())
                })
        })
    })
}

pub(crate) fn extract_bool_field(value: &Value, field_names: &[&str]) -> Option<bool> {
    field_names.iter().find_map(|field_name| {
        value.get(*field_name).and_then(|item| {
            item.as_bool()
                .or_else(|| item.as_i64().map(|number| number != 0))
        })
    })
}

pub(crate) fn extract_f64_field(value: &Value, field_names: &[&str]) -> Option<f64> {
    field_names.iter().find_map(|field_name| {
        value.get(*field_name).and_then(|item| {
            item.as_f64().or_else(|| {
                item.as_str()
                    .and_then(|text| text.trim().parse::<f64>().ok())
            })
        })
    })
}

pub(crate) fn trim_message(value: &str) -> String {
    const MAX_LEN: usize = 300;
    let text = value.trim();
    // 按字符（而非字节）截断：HTTP 错误页常含中文/多字节字符，按字节切片会在字符边界中间 panic。
    let truncated: String = text.chars().take(MAX_LEN).collect();
    if truncated.len() < text.len() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::parse_success_data;
    use reqwest::StatusCode;

    #[test]
    fn rejects_success_response_without_success_flag() {
        let error = parse_success_data(
            &StatusCode::OK,
            r#"{"data": {"id": 1}}"#.to_string(),
            "测试",
        )
        .expect_err("missing success must not be accepted");
        assert!(error.contains("缺少 success/code"));
    }

    #[test]
    fn unwraps_explicit_success_data() {
        let value = parse_success_data(
            &StatusCode::OK,
            r#"{"success":true,"data":{"id":1}}"#.to_string(),
            "测试",
        )
        .expect("success response should parse");
        assert_eq!(value["id"], 1);
    }

    #[test]
    fn accepts_token_usage_code_flag() {
        let value = parse_success_data(
            &StatusCode::OK,
            r#"{"code":true,"data":{"total_available":10}}"#.to_string(),
            "API 密钥额度",
        )
        .expect("token usage response should parse");
        assert_eq!(value["total_available"], 10);
    }

    #[test]
    fn rejects_false_token_usage_code_flag() {
        let error = parse_success_data(
            &StatusCode::OK,
            r#"{"code":false,"message":"无效密钥"}"#.to_string(),
            "API 密钥额度",
        )
        .expect_err("failed token usage response must be rejected");
        assert!(error.contains("无效密钥"));
    }
}
