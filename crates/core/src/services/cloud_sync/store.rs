//! Remote object storage shared by the sync engine. Every backend must provide
//! strong version identifiers and conditional writes: those are the only
//! cross-device lock, so a store that ignores them is rejected during probing.
use super::format::random_id;

#[derive(Clone)]
pub(crate) enum Download {
    Missing,
    Unchanged,
    Found { bytes: Vec<u8>, etag: String },
}

#[async_trait::async_trait]
pub(crate) trait RemoteStore: Send + Sync {
    fn label(&self) -> &'static str;

    async fn get(
        &self,
        file: &str,
        if_none_match: Option<&str>,
        limit: usize,
    ) -> Result<Download, String>;

    /// false means the remote changed; never retry this write without re-reading.
    async fn put(&self, file: &str, bytes: Vec<u8>, expected: Option<&str>)
        -> Result<bool, String>;

    async fn remove(&self, file: &str) -> Result<(), String>;

    /// Backend-specific preparation before the probe (WebDAV directories).
    async fn prepare(&self) -> Result<(), String> {
        Ok(())
    }

    /// Check the actual read/write and conditional-write contract using only
    /// a random temporary object; never change the user's current snapshot.
    async fn test(&self) -> Result<(), String>;
}

pub(super) async fn probe_store(store: &dyn RemoteStore) -> Result<(), String> {
    store.prepare().await?;
    let name = format!(".balancehub-probe-{}", random_id()?);
    let result = probe(store, &name).await;
    let cleanup = store.remove(&name).await;
    result?;
    cleanup.map_err(|_| {
        format!(
            "连接验证通过，但临时探测文件清理失败，请检查{}的删除权限",
            store.label()
        )
    })
}

async fn probe(store: &dyn RemoteStore, name: &str) -> Result<(), String> {
    let content = b"BalanceHub capability probe".to_vec();
    if !store.put(name, content.clone(), None).await? {
        return Err(format!("{}不支持创建同步文件", store.label()));
    }
    let mut etag = match store.get(name, None, 1024).await? {
        Download::Found { bytes, etag } if bytes == content => etag,
        _ => return Err(format!("{}写入后读取内容不一致", store.label())),
    };
    if store
        .put(name, content.clone(), Some("\"balancehub-invalid-etag\""))
        .await?
        || store.put(name, content.clone(), None).await?
    {
        return Err(format!(
            "该{}服务忽略条件写入，无法防止多设备互相覆盖，请使用支持 If-Match 的服务",
            store.label()
        ));
    }
    // A timestamp/size ETag can claim to be strong while missing rapid
    // same-size updates (including encrypted manifests). Check this before
    // trusting either conditional reads or writes for user data.
    for marker in *b"123" {
        let mut replacement = content.clone();
        replacement[0] = marker;
        if !store.put(name, replacement.clone(), Some(&etag)).await? {
            return Err(format!("{}条件写入验证失败", store.label()));
        }
        let next_etag = match store.get(name, None, 1024).await? {
            Download::Found { bytes, etag: next } if bytes == replacement && next != etag => next,
            Download::Found { bytes, .. } if bytes == replacement => {
                return Err(format!(
                    "{}的文件版本标识未随内容变化，无法安全同步；请使用提供可靠强 ETag 的服务",
                    store.label()
                ))
            }
            _ => return Err(format!("{}条件写入后内容不一致", store.label())),
        };
        if store.put(name, content.clone(), Some(&etag)).await? {
            return Err(format!(
                "{}接受了过期的文件版本，无法安全协调多设备同步",
                store.label()
            ));
        }
        match store.get(name, Some(&etag), 1024).await? {
            Download::Found { bytes, .. } if bytes == replacement => {}
            _ => {
                return Err(format!(
                    "{}条件读取未返回最新内容，无法安全同步",
                    store.label()
                ))
            }
        }
        etag = next_etag;
    }
    Ok(())
}
