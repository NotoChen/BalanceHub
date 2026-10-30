use crate::{adapters::oauth::tokens, models::*, services::oauth_accounts::native::iso};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(crate) fn options(auth: &ProviderAuth) -> Vec<OAuthExportOption> {
    let Some(identity) = &auth.oauth else {
        return Vec::new();
    };
    [
        (OAuthExportFormat::BalanceHub, "BalanceHub", "在 BalanceHub 的导入授权文件中选择此文件。"),
        (OAuthExportFormat::Cpa, "CPA", "在 CPA 管理面板的授权文件中上传；也可从 BalanceHub 发送到已连接的 CPA。"),
        (OAuthExportFormat::CcSwitch, "CC Switch", "这是 CC Switch 的账号存储格式。退出 CC Switch 后，将 accounts 条目合并到同名文件，保留原有账号，再启动 CC Switch。"),
        (OAuthExportFormat::NewApi, "NewAPI", "在 NewAPI 管理员的 Codex 渠道中，将此 JSON 作为渠道密钥导入。"),
        (OAuthExportFormat::Sub2Api, "Sub2API", "在 Sub2API 管理员的账号数据导入中选择此文件，再设置该站点的分组、代理和并发。"),
        (OAuthExportFormat::Magpie, "Magpie", "在 Magpie 对应账号的导入授权文件入口选择此文件。"),
        (OAuthExportFormat::Native, "原生 CLI", "在目标 CLI 退出后使用此文件；不要覆盖其他账号，也不要让多个应用同时续期同一授权。"),
    ].into_iter().map(|(format, label, instructions)| {
        let reason = unavailable_reason(auth, identity, format);
        OAuthExportOption { format, label, available: reason.is_none(), reason, instructions, file_name: file_name(identity.vendor, format) }
    }).collect()
}

fn file_name(vendor: OAuthVendor, format: OAuthExportFormat) -> String {
    match (format, vendor) {
        (OAuthExportFormat::CcSwitch, OAuthVendor::Codex) => "codex_oauth_auth.json".into(),
        (OAuthExportFormat::CcSwitch, OAuthVendor::Grok) => "xai_oauth_auth.json".into(),
        (OAuthExportFormat::Native, vendor) => {
            crate::services::oauth_accounts::native::file_name(vendor).into()
        }
        _ => format!("{}-oauth.json", vendor.key()),
    }
}

fn unavailable_reason(
    auth: &ProviderAuth,
    identity: &OAuthCredential,
    format: OAuthExportFormat,
) -> Option<&'static str> {
    if auth.access_token.is_empty() && auth.refresh_token.is_empty() {
        return Some("此设备只有账号信息，请先授权或导入原始授权文件");
    }
    if identity.refresh_pending {
        return Some("账号正在续期或上次续期结果未确认，请完成授权后再导出");
    }
    if format == OAuthExportFormat::BalanceHub {
        return None;
    }
    // An empty client id means the credential never recorded one; only a
    // different recorded client proves the token belongs to another app.
    if !identity.client_id.is_empty() && identity.client_id != tokens::client_id(identity.vendor) {
        return Some(
            "这份授权是用其他应用的 OAuth 客户端签发的，目标应用无法用它续期；请在目标应用中重新登录该账号",
        );
    }
    let vendor = identity.vendor;
    match format {
        OAuthExportFormat::Cpa if vendor == OAuthVendor::Gemini => {
            return Some("当前 CPA 的 Google 授权使用 Antigravity，与 Gemini CLI 授权不同")
        }
        OAuthExportFormat::CcSwitch
            if !matches!(vendor, OAuthVendor::Codex | OAuthVendor::Grok) =>
        {
            return Some("CC Switch 仅提供 Codex / Grok 的独立 OAuth 账号存储")
        }
        OAuthExportFormat::NewApi if vendor != OAuthVendor::Codex => {
            return Some("NewAPI 仅在 Codex 渠道使用此独立 OAuth 文件格式")
        }
        OAuthExportFormat::Magpie
            if !matches!(vendor, OAuthVendor::Codex | OAuthVendor::Claude) =>
        {
            return Some("Magpie 内建文件导入支持 Codex / Claude；Gemini / Grok 由独立插件管理")
        }
        _ => {}
    }
    if matches!(
        format,
        OAuthExportFormat::CcSwitch | OAuthExportFormat::Magpie
    ) && auth.refresh_token.is_empty()
    {
        return Some("目标应用需要刷新令牌，请先重新授权");
    }
    if format == OAuthExportFormat::Magpie
        && vendor == OAuthVendor::Claude
        && identity.email.is_empty()
    {
        return Some("Magpie 的 Claude 导入需要账号邮箱，请先刷新账号信息或重新授权");
    }
    if matches!(
        format,
        OAuthExportFormat::Native | OAuthExportFormat::NewApi
    ) && auth.access_token.is_empty()
    {
        return Some("此文件只有刷新令牌，请先由本机续期取得访问令牌再导出");
    }
    if vendor == OAuthVendor::Codex
        && matches!(
            format,
            OAuthExportFormat::Native | OAuthExportFormat::CcSwitch
        )
        && (identity.id_token.is_empty() || identity.account_id.is_empty())
    {
        return Some("此格式需要完整的 ID Token 和账号工作区，请先重新授权");
    }
    if vendor == OAuthVendor::Grok
        && format == OAuthExportFormat::CcSwitch
        && identity.subject.is_empty()
    {
        return Some("CC Switch 需要账号身份 ID，请先重新授权");
    }
    if format == OAuthExportFormat::Native
        && vendor == OAuthVendor::Claude
        && (auth.access_token_expires_at.is_none() || identity.scopes.is_empty())
    {
        return Some("Claude Code 原生文件需要到期时间和授权范围，请先重新授权");
    }
    None
}

fn sub2_account(provider: &Provider, identity: &OAuthCredential, mut credentials: Value) -> Value {
    let vendor = identity.vendor;
    credentials["expires_at"] = iso(provider.auth.access_token_expires_at);
    credentials["scope"] = identity.scopes.join(" ").into();
    credentials["token_type"] = "Bearer".into();
    match vendor {
        OAuthVendor::Codex => {
            credentials["chatgpt_account_id"] = identity.account_id.clone().into();
            credentials["chatgpt_user_id"] = identity.subject.clone().into();
            credentials["organization_id"] = identity.organization_id.clone().into();
        }
        OAuthVendor::Claude => {
            credentials["account_uuid"] = identity.subject.clone().into();
            credentials["org_uuid"] = identity.organization_id.clone().into();
        }
        OAuthVendor::Grok => {
            credentials["team_id"] = identity.organization_id.clone().into();
            credentials["subscription_tier"] = identity.plan.clone().into();
        }
        OAuthVendor::Gemini => credentials["oauth_type"] = "code_assist".into(),
    }
    json!({"name":provider.display_label(),"platform":match vendor{OAuthVendor::Codex=>"openai",OAuthVendor::Claude=>"anthropic",OAuthVendor::Gemini=>"gemini",OAuthVendor::Grok=>"grok"},"type":"oauth","credentials":credentials,"concurrency":1,"priority":50})
}

/// The one-account balancehub-oauth payload shared by file export and the
/// in-app credential editor, so both always show the same fields.
fn raw_account(provider: &Provider, identity: &OAuthCredential) -> Value {
    let auth = &provider.auth;
    let vendor = identity.vendor;
    let mut raw = json!({"type":if vendor==OAuthVendor::Grok{"xai"}else{vendor.key()},"access_token":auth.access_token,"refresh_token":auth.refresh_token,"id_token":identity.id_token,"account_id":identity.account_id,"email":identity.email,"sub":identity.subject,"account_uuid":identity.subject,"organization_uuid":identity.organization_id,"organization_name":identity.organization_name,"project_id":identity.project_id,"client_id":identity.client_id,"scopes":identity.scopes,"plan_type":identity.plan,"expired":iso(auth.access_token_expires_at),"last_refresh":iso(identity.updated_at)});
    if !identity.client_secret.is_empty() {
        raw["client_secret"] = identity.client_secret.clone().into();
    }
    raw
}

/// In-app view of the credential BalanceHub stores. Unlike a file handoff this
/// stays available while a refresh is pending, because that is exactly when the
/// stored tokens need inspecting.
pub(crate) fn source(provider: &Provider) -> Result<String, String> {
    let identity = provider
        .auth
        .oauth
        .as_ref()
        .ok_or("此卡片不是 OAuth 账号")?;
    serde_json::to_string_pretty(&json!({
        "format":"balancehub-oauth",
        "version":1,
        "accounts":[raw_account(provider, identity)],
    }))
    .map_err(|_| "生成授权原文失败".into())
}

pub(crate) fn export(
    provider: &Provider,
    format: OAuthExportFormat,
) -> Result<OAuthExportFile, String> {
    let auth = &provider.auth;
    let identity = auth.oauth.as_ref().ok_or("此卡片不是 OAuth 账号")?;
    let option = options(auth)
        .into_iter()
        .find(|v| v.format == format)
        .ok_or("导出格式无效")?;
    if !option.available {
        return Err(option.reason.unwrap_or("目标应用不支持此授权").into());
    }
    if identity.refresh_pending {
        return Err("账号正在续期，请等待结果保存后再导出".into());
    }
    let vendor = identity.vendor;
    let raw = raw_account(provider, identity);
    let hash = format!("{:x}", Sha256::digest(provider.identity.id.as_bytes()));
    let name = option.file_name;
    let mut instructions = option.instructions.to_owned();
    let output = match format {
        // Magpie's flat Claude reader omits account/org UUIDs. Its Sub2API reader
        // preserves both, so use that accepted shape for a complete handoff.
        OAuthExportFormat::Magpie if vendor == OAuthVendor::Claude => {
            sub2_account(provider, identity, raw)
        }
        OAuthExportFormat::Cpa | OAuthExportFormat::NewApi | OAuthExportFormat::Magpie => raw,
        OAuthExportFormat::BalanceHub => {
            json!({"format":"balancehub-oauth","version":1,"accounts":[raw]})
        }
        OAuthExportFormat::Sub2Api => {
            json!({"type":"sub2api-data","version":1,"exported_at":chrono::Utc::now().to_rfc3339(),"proxies":[],"accounts":[sub2_account(provider, identity, raw)]})
        }
        OAuthExportFormat::CcSwitch => {
            let local_id = if vendor == OAuthVendor::Grok {
                identity.subject.clone()
            } else {
                format!("bh-{}", &hash[..24])
            };
            let account = if vendor == OAuthVendor::Codex {
                json!({"account_id":local_id,"chatgpt_account_id":identity.account_id,"email":identity.email,"refresh_token":auth.refresh_token,"authenticated_at":identity.updated_at.unwrap_or(chrono::Utc::now().timestamp()),"id_token":identity.id_token,"token_updated_at_ms":identity.updated_at.unwrap_or(0)*1000})
            } else {
                json!({"account_id":local_id,"login":identity.email,"refresh_token":auth.refresh_token,"authenticated_at":identity.updated_at.unwrap_or(chrono::Utc::now().timestamp()),"requires_reauth":false})
            };
            json!({"version":if vendor==OAuthVendor::Codex{2}else{1},"accounts":{local_id.clone():account},"default_account_id":local_id})
        }
        OAuthExportFormat::Native => {
            instructions.push_str(match vendor {
                OAuthVendor::Codex => {
                    " 对应 CODEX_HOME/auth.json；使用系统钥匙串的安装应通过原生登录导入。"
                }
                OAuthVendor::Claude => {
                    " 对应 Claude Code 的 .credentials.json；macOS 钥匙串安装应使用原生登录。"
                }
                OAuthVendor::Gemini => " 对应 .gemini/oauth_creds.json。",
                OAuthVendor::Grok => " 对应 .grok/auth.json，保留其他 issuer 条目。",
            });
            crate::services::oauth_accounts::native::document(vendor, auth, identity, true)
        }
    };
    Ok(OAuthExportFile {
        name,
        content: serde_json::to_string_pretty(&output).map_err(|_| "生成授权文件失败")?,
        instructions,
    })
}
