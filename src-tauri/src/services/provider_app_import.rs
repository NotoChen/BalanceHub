//! Outgoing provider links: Magpie v0.1.1285 importurl.go and CC Switch V1.
//! This module only projects saved settings; it performs no network or file IO.

use reqwest::Url;
use sha2::{Digest, Sha256};
use std::net::IpAddr;

use crate::models::{
    provider_domain::auth, AgentCliKind, CcSwitchAgent, Provider, ProviderAppImportOption,
    ProviderAppImportTarget,
};

use super::agent_cli;

pub(crate) fn options(provider: &Provider) -> Vec<ProviderAppImportOption> {
    std::iter::once(ProviderAppImportTarget::Magpie)
        .chain(CcSwitchAgent::ALL.map(ProviderAppImportTarget::CcSwitch))
        .map(|target| ProviderAppImportOption {
            target,
            label: target.label(),
            unavailable_reason: import_settings(provider, target).err(),
            note: if matches!(target, ProviderAppImportTarget::Magpie)
                && !auth::has_api_key(provider)
            {
                Some("可在 Magpie 中补填 API Key")
            } else {
                None
            },
        })
        .collect()
}

struct ImportSettings<'a> {
    api_key: &'a str,
    endpoints: Vec<(&'static str, String)>,
}

pub(crate) fn build_link(
    provider: &Provider,
    target: ProviderAppImportTarget,
) -> Result<String, String> {
    let settings = import_settings(provider, target)?;
    let label = provider.display_label();
    let name = if label.is_empty() {
        let display_name = provider.identity.display_name.trim();
        if display_name.is_empty() {
            "中转站"
        } else {
            display_name
        }
    } else {
        &label
    };
    let mut url = Url::parse(match target {
        ProviderAppImportTarget::Magpie => "magpie://import",
        ProviderAppImportTarget::CcSwitch(_) => "ccswitch://v1/import",
    })
    .map_err(|_| "无法生成应用导入链接".to_string())?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("name", name);
        match target {
            ProviderAppImportTarget::Magpie => {
                // Magpie's name slug is ASCII-only. A stable card identity also
                // keeps renames/key rotation on its explicit Replace path.
                let digest = format!("{:x}", Sha256::digest(provider.identity.id.as_bytes()));
                query.append_pair("id", &format!("balancehub-{}", &digest[..32]));
                if !settings.api_key.is_empty() {
                    query.append_pair("key", settings.api_key);
                }
                // Magpie's models field restricts exposed models. Let it fetch
                // with this key instead of exporting an account/default model.
            }
            ProviderAppImportTarget::CcSwitch(agent) => {
                query
                    .append_pair("resource", "provider")
                    .append_pair("app", agent.id())
                    .append_pair("apiKey", settings.api_key);
                let model = provider.cli.preferred_model.trim();
                if !model.is_empty() {
                    query.append_pair("model", model);
                }
            }
        }
        for (field, endpoint) in settings.endpoints {
            query.append_pair(field, &endpoint);
        }
    }
    Ok(url.into())
}

fn import_settings(
    provider: &Provider,
    target: ProviderAppImportTarget,
) -> Result<ImportSettings<'_>, String> {
    let api_key = if auth::has_api_key(provider) {
        provider.auth.api_key.trim()
    } else {
        ""
    };
    if api_key.chars().any(|value| value.is_ascii_control()) {
        return Err("API Key 包含换行或控制字符，请先修正".to_string());
    }
    let endpoints = match target {
        ProviderAppImportTarget::Magpie => {
            if provider.identity.id.trim().is_empty() {
                return Err("请先保存中转站".to_string());
            }
            if api_key.len() > 4096 {
                return Err("API Key 超过 Magpie 支持的 4096 字节上限".to_string());
            }
            magpie_endpoints(provider)?
        }
        ProviderAppImportTarget::CcSwitch(agent) => {
            if api_key.is_empty() {
                return Err("请先为中转站设置完整的 API Key".to_string());
            }
            let kind = match agent {
                CcSwitchAgent::Claude => AgentCliKind::ClaudeCode,
                CcSwitchAgent::Gemini => AgentCliKind::Gemini,
                CcSwitchAgent::Grokbuild => AgentCliKind::Grok,
                _ => AgentCliKind::Codex,
            };
            let endpoint = native_base_url(kind, provider);
            vec![(
                "endpoint",
                validate_endpoint(&endpoint, target, "接口地址")?,
            )]
        }
    };
    Ok(ImportSettings { api_key, endpoints })
}

fn magpie_endpoints(provider: &Provider) -> Result<Vec<(&'static str, String)>, String> {
    let target = ProviderAppImportTarget::Magpie;
    // API-key provider imports expose an OpenAI-compatible gateway. OAuth
    // credentials use the separate authorization-file exchange workflow.
    // A Grok override is explicitly the Chat endpoint, never a Codex override.
    let mut endpoints = vec![(
        "chat",
        validate_endpoint(
            &native_base_url(AgentCliKind::Grok, provider),
            target,
            "OpenAI 接口地址",
        )?,
    )];
    // Other native protocols are exported only when explicitly configured.
    // A site type or a model name alone does not prove protocol support.
    for (kind, field, label) in [
        (AgentCliKind::Codex, "responses", "Responses 接口地址"),
        (AgentCliKind::ClaudeCode, "anthropic", "Anthropic 接口地址"),
        (AgentCliKind::Gemini, "gemini", "Gemini 接口地址"),
    ] {
        if provider
            .liveness
            .agent_base_urls
            .get(&kind)
            .is_none_or(|value| value.trim().is_empty())
        {
            continue;
        }
        let base = native_base_url(kind, provider);
        let endpoint = match kind {
            AgentCliKind::Gemini => format!("{base}/v1beta"),
            _ => base,
        };
        endpoints.push((field, validate_endpoint(&endpoint, target, label)?));
    }
    Ok(endpoints)
}

fn native_base_url(kind: AgentCliKind, provider: &Provider) -> String {
    let raw = agent_cli::provider_raw_base_url(kind, provider).trim_end_matches('/');
    // BalanceHub also accepts a complete models URL. Deep links need an API
    // base, or the receiving app would append paths after /models or /messages.
    let suffixes: &[&str] = match kind {
        AgentCliKind::ClaudeCode => &["/v1/messages", "/messages", "/models"],
        AgentCliKind::Gemini => &["/models"],
        _ => &["/chat/completions", "/responses", "/models"],
    };
    let suffix = Url::parse(raw).ok().and_then(|url| {
        if url.query().is_some() || url.fragment().is_some() {
            return None;
        }
        suffixes
            .iter()
            .copied()
            .find(|suffix| url.path().ends_with(suffix))
    });
    let base = suffix
        .and_then(|suffix| raw.strip_suffix(suffix))
        .unwrap_or(raw);
    // An explicit /models or /responses URL already identifies the API base,
    // including unversioned or /v2 gateways. Do not append a guessed /v1.
    if suffix.is_some() && matches!(kind, AgentCliKind::Codex | AgentCliKind::Grok) {
        return base.to_string();
    }
    let base = if matches!(kind, AgentCliKind::ClaudeCode) {
        base.strip_suffix("/v1").unwrap_or(base)
    } else {
        base
    };
    agent_cli::normalize_base_url(kind, base)
}

fn validate_endpoint(
    raw: &str,
    target: ProviderAppImportTarget,
    label: &str,
) -> Result<String, String> {
    let url = Url::parse(raw).map_err(|_| format!("{label}无效，请先修改中转站配置"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || raw.chars().any(|value| value.is_ascii_control())
    {
        return Err(format!(
            "{label}需要是无账号、查询参数或锚点的 HTTP(S) 地址"
        ));
    }
    if matches!(target, ProviderAppImportTarget::Magpie)
        && url.scheme() == "http"
        && !is_magpie_local_host(&url)
    {
        return Err(format!(
            "Magpie 要求{label}使用 HTTPS，本机和局域网地址可用 HTTP"
        ));
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

fn is_magpie_local_host(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default();
    if host == "localhost" || host.ends_with(".local") {
        return true;
    }
    match host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    {
        Ok(IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private(),
        Ok(IpAddr::V6(ip)) => {
            ip.is_loopback()
                || ip.octets()[0] & 0xfe == 0xfc
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|ip| ip.is_loopback() || ip.is_private())
        }
        Err(_) => false,
    }
}
