use super::{interchange, random_id};
use crate::models::*;
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

pub(crate) struct PreparedEntry {
    pub(crate) id: String,
    pub(crate) auth: ProviderAuth,
    pub(crate) existing: Option<(String, u64)>,
}
struct Preview {
    expires: Instant,
    entries: Vec<PreparedEntry>,
}
fn previews() -> &'static Mutex<HashMap<String, Preview>> {
    static STATE: OnceLock<Mutex<HashMap<String, Preview>>> = OnceLock::new();
    STATE.get_or_init(Mutex::default)
}

pub(crate) fn prepare(
    files: Vec<OAuthImportFile>,
    hint: Option<OAuthVendor>,
    existing: &[Provider],
) -> Result<OAuthImportPreview, String> {
    if files.is_empty()
        || files.len() > 64
        || files.iter().map(|f| f.content.len()).sum::<usize>() > 8 * 1024 * 1024
    {
        return Err("请选择最多 64 个授权文件，总大小不超过 8 MiB".into());
    }
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let parsed = match interchange::parse_file(file, hint) {
            Ok(entries) => entries,
            Err(error) => {
                errors.push(format!("文件 {}：{error}", index + 1));
                continue;
            }
        };
        for entry in parsed {
            match entry {
                Err(error) => errors.push(format!("文件 {}：{error}", index + 1)),
                Ok(auth) => {
                    if entries
                        .iter()
                        .any(|entry: &PreparedEntry| interchange::same_account(&entry.auth, &auth))
                    {
                        continue;
                    }
                    let duplicate = existing
                        .iter()
                        .find(|p| interchange::same_account(&p.auth, &auth));
                    if entries.len() >= crate::limits::MAX_PROVIDERS {
                        return Err("一次导入的账号数量超过卡片上限，请分批导入".into());
                    }
                    entries.push(PreparedEntry {
                        id: random_id()?,
                        auth,
                        existing: duplicate
                            .map(|p| (p.identity.id.clone(), p.auth.credential_revision)),
                    });
                }
            }
        }
    }
    let public = entries
        .iter()
        .map(|entry| {
            let identity = entry.auth.oauth.as_ref().expect("validated");
            OAuthImportEntry {
                id: entry.id.clone(),
                vendor: identity.vendor,
                label: if identity.email.is_empty() {
                    identity.vendor.label().into()
                } else {
                    identity.email.clone()
                },
                account_id: identity.account_id.clone(),
                plan: identity.plan.clone(),
                source: identity.source.clone(),
                expires_at: entry.auth.access_token_expires_at,
                refreshable: !entry.auth.refresh_token.is_empty(),
                duplicate_id: entry.existing.as_ref().map(|(id, _)| id.clone()),
            }
        })
        .collect();
    let token = random_id()?;
    let mut state = previews().lock().map_err(|_| "授权预览状态不可用")?;
    state.retain(|_, v| v.expires > Instant::now());
    if state.len() >= 8 {
        return Err("尚未完成的授权导入过多，请关闭旧预览后再试".into());
    }
    state.insert(
        token.clone(),
        Preview {
            expires: Instant::now() + Duration::from_secs(10 * 60),
            entries,
        },
    );
    Ok(OAuthImportPreview {
        token,
        entries: public,
        errors,
    })
}

pub(crate) fn discard(token: &str) {
    if let Ok(mut state) = previews().lock() {
        state.remove(token);
    }
}
pub(crate) fn consume(request: &OAuthImportRequest) -> Result<Vec<PreparedEntry>, String> {
    let ids: HashSet<_> = request.entry_ids.iter().collect();
    if ids.is_empty() || ids.len() != request.entry_ids.len() {
        return Err("请选择需要导入的账号".into());
    }
    let mut state = previews().lock().map_err(|_| "授权预览状态不可用")?;
    let preview = state
        .get(&request.token)
        .filter(|p| p.expires > Instant::now())
        .ok_or("授权预览已过期，请重新读取文件")?;
    if !ids
        .iter()
        .all(|id| preview.entries.iter().any(|e| &e.id == *id))
    {
        return Err("导入选择已变化，请重新预览".into());
    }
    let preview = state.remove(&request.token).ok_or("授权预览已失效")?;
    Ok(preview
        .entries
        .into_iter()
        .filter(|e| ids.contains(&e.id))
        .collect())
}
