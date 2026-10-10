use super::{
    crypto::{parse_head, Cipher},
    files,
    format::{
        self, Baseline, Entry, Head, Manifest, SyncDocument, SyncDocuments, MAX_HEAD_BYTES,
        MAX_OBJECT_BYTES, VERSION,
    },
    merge,
    transport::{DavClient, Download},
};
use crate::models::{CloudSyncPhase, CloudSyncResolution, CloudSyncReview};
use futures_util::{stream, StreamExt, TryStreamExt};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

pub(super) trait Replica: Send + Sync {
    fn snapshot(&self) -> Result<SyncDocuments, String>;
    fn validate(&self, documents: &SyncDocuments) -> Result<(), String>;
    fn apply(&self, expected: &SyncDocuments, desired: &SyncDocuments) -> Result<(), String>;
    fn recovery(&self) -> Result<Option<SyncDocuments>, String>;
}

#[derive(Clone, Debug, Default)]
pub(super) struct TransferStats {
    pub uploaded: usize,
    pub downloaded: usize,
    pub bytes: u64,
    pub done: usize,
    pub total: usize,
}

pub(super) type Progress = Arc<dyn Fn(CloudSyncPhase, &str, TransferStats) + Send + Sync>;

pub(super) struct Control {
    state: AtomicU8,
    pub cancelled: tokio::sync::Notify,
    started: Instant,
}

impl Default for Control {
    fn default() -> Self {
        Self {
            state: AtomicU8::new(0),
            cancelled: tokio::sync::Notify::new(),
            started: Instant::now(),
        }
    }
}

impl Control {
    pub fn cancel(&self) -> bool {
        if self
            .state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.cancelled.notify_one();
            true
        } else {
            false
        }
    }
    pub fn is_cancelled(&self) -> bool {
        self.state.load(Ordering::Acquire) == 1
    }
    pub fn can_cancel(&self) -> bool {
        self.state.load(Ordering::Acquire) == 0
    }
    pub fn expire(&self) -> bool {
        self.state
            .compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    pub fn begin_commit(&self) -> Result<(), String> {
        self.check()?;
        self.state
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| "同步已取消".to_owned())
    }
    pub fn check(&self) -> Result<(), String> {
        if self.is_cancelled() {
            return Err("同步已取消".to_owned());
        }
        if self.started.elapsed() > Duration::from_secs(900) {
            return Err(
                "本次同步超过 15 分钟，已停止；可以重试，完整的本地数据仍然可用".to_owned(),
            );
        }
        Ok(())
    }
}

pub(super) struct Engine {
    pub root: PathBuf,
    pub dav: DavClient,
    pub passphrase: String,
    pub device: String,
    pub replica: Arc<dyn Replica>,
    pub progress: Progress,
    pub control: Arc<Control>,
}

pub(super) struct Prepared {
    pub id: String,
    pub local: SyncDocuments,
    pub remote: SyncDocuments,
    pub manifest: Manifest,
    pub baseline: Baseline,
    pub head: Option<Head>,
    pub etag: Option<String>,
    pub cipher: Arc<Cipher>,
    pub stats: TransferStats,
}

impl Prepared {
    pub fn review(&self) -> Result<CloudSyncReview, String> {
        let merged = merge::merge(
            &self.baseline,
            &self.local,
            &self.remote,
            &self.manifest,
            &[],
        )?;
        Ok(CloudSyncReview {
            id: self.id.clone(),
            initial: !self.baseline.initialized,
            remote_device: self.manifest.device.clone(),
            remote_updated_at: (self.manifest.updated_at > 0).then_some(self.manifest.updated_at),
            changes: merged.changes,
        })
    }
}

impl Engine {
    pub async fn remote_unchanged(&self) -> Result<bool, String> {
        let baseline: Baseline =
            files::read_json(&self.root.join("baseline.json"))?.unwrap_or_default();
        let Some(etag) = baseline
            .etag
            .as_deref()
            .filter(|_| baseline.initialized && baseline.head.is_some())
        else {
            return Ok(false);
        };
        match self
            .dav
            .get("head.json", Some(etag), MAX_HEAD_BYTES)
            .await?
        {
            Download::Unchanged => Ok(true),
            Download::Found { .. } => Ok(false),
            Download::Missing => {
                Err("云端同步清单已被移除，请恢复云端清单或选择新的同步目录".to_owned())
            }
        }
    }

    pub async fn snapshot(&self) -> Result<SyncDocuments, String> {
        let replica = Arc::clone(&self.replica);
        tauri::async_runtime::spawn_blocking(move || replica.snapshot())
            .await
            .map_err(|_| "读取同步数据的后台任务异常".to_owned())?
    }

    pub async fn prepare(&self) -> Result<Prepared, String> {
        self.control.check()?;
        (self.progress)(
            CloudSyncPhase::Preparing,
            "正在比较本地与云端配置",
            TransferStats::default(),
        );
        let local = self.snapshot().await?;
        let baseline: Baseline =
            files::read_json(&self.root.join("baseline.json"))?.unwrap_or_default();
        let cached_etag = baseline.head.as_ref().and(baseline.etag.as_deref());
        let (head, etag) = match self
            .dav
            .get("head.json", cached_etag, MAX_HEAD_BYTES)
            .await?
        {
            Download::Found { bytes, etag } => (Some(parse_head(&bytes)?), Some(etag)),
            Download::Unchanged => (baseline.head.clone(), baseline.etag.clone()),
            Download::Missing if baseline.initialized => return Err(
                "云端同步清单已被移除，本地数据未改变。请恢复云端清单，或选择新的同步目录重新连接"
                    .to_owned(),
            ),
            Download::Missing => (None, None),
        };
        self.control.check()?;
        let passphrase = self.passphrase.clone();
        let salt = head.as_ref().map(|value| value.salt.clone());
        let cipher = Arc::new(
            tauri::async_runtime::spawn_blocking(move || {
                Cipher::derive(&passphrase, salt.as_deref())
            })
            .await
            .map_err(|_| "生成同步密钥失败")??,
        );
        let manifest = match &head {
            Some(head) => cipher.decode_head(head)?,
            None => Manifest {
                version: VERSION,
                ..Manifest::default()
            },
        };
        let mut remote = BTreeMap::new();
        let mut downloads = Vec::new();
        for (key, entry) in &manifest.entries {
            if !valid_key(key) {
                return Err("云端包含此版本不支持的同步条目，请升级 BalanceHub".to_owned());
            }
            if entry.object.is_none() {
                continue;
            }
            if let Some(document) = local
                .get(key)
                .filter(|doc| doc.hash().ok().as_ref() == Some(&entry.hash))
            {
                remote.insert(key.clone(), document.clone());
            } else {
                downloads.push((key.clone(), entry.clone()));
            }
        }
        let total = downloads.len();
        let mut stats = TransferStats {
            total,
            ..TransferStats::default()
        };
        let mut futures = stream::iter(downloads)
            .map(|(key, entry)| {
                let cipher = Arc::clone(&cipher);
                async move {
                    self.control.check()?;
                    let (document, bytes) = self.load_document(&entry, &cipher).await?;
                    Ok::<_, String>((key, document, bytes))
                }
            })
            .buffer_unordered(4);
        while let Some((key, document, bytes)) = futures.try_next().await? {
            remote.insert(key, document);
            stats.done += 1;
            if bytes > 0 {
                stats.downloaded += 1;
                stats.bytes += bytes;
            }
            (self.progress)(
                CloudSyncPhase::Downloading,
                "正在获取变化的配置与资源",
                stats.clone(),
            );
        }
        drop(futures);
        self.control.check()?;
        Ok(Prepared {
            id: format::random_id()?,
            local,
            remote,
            manifest,
            baseline,
            head,
            etag,
            cipher,
            stats,
        })
    }

    async fn load_document(
        &self,
        entry: &Entry,
        cipher: &Cipher,
    ) -> Result<(SyncDocument, u64), String> {
        let object = entry.object.as_deref().ok_or("同步对象缺失")?;
        if !format::valid_object_id(object) {
            return Err("云端对象标识无效".to_owned());
        }
        let path = self.root.join("cache").join(object);
        let cached = std::fs::read(&path)
            .ok()
            .filter(|bytes| format::digest(bytes) == object);
        let (bytes, transferred) = if let Some(bytes) = cached {
            (bytes, 0)
        } else {
            match self
                .dav
                .get(&format!("objects/{object}"), None, MAX_OBJECT_BYTES)
                .await?
            {
                Download::Found { bytes, .. } => {
                    if format::digest(&bytes) != object {
                        return Err("云端文件校验失败，未应用同步数据".to_owned());
                    }
                    let count = bytes.len() as u64;
                    files::write_bytes(&path, &bytes)?;
                    (bytes, count)
                }
                _ => return Err("云端快照引用的资源缺失，未应用同步数据".to_owned()),
            }
        };
        let plain = cipher.decrypt(&bytes, b"balancehub-record-v1")?;
        let document: SyncDocument =
            serde_json::from_slice(&plain).map_err(|_| "云端配置格式无效")?;
        if document.hash()? != entry.hash {
            return Err("云端配置内容校验失败".to_owned());
        }
        Ok((document, transferred))
    }

    pub async fn verify_fresh(&self, prepared: &Prepared) -> Result<(), String> {
        self.control.check()?;
        if self.snapshot().await? != prepared.local {
            return Err("本地配置在预览后已变化，请重新同步获取最新差异".to_owned());
        }
        match self
            .dav
            .get("head.json", prepared.etag.as_deref(), MAX_HEAD_BYTES)
            .await?
        {
            Download::Unchanged if prepared.etag.is_some() => Ok(()),
            Download::Missing if prepared.head.is_none() => Ok(()),
            _ => Err("云端配置在预览后已变化，请重新同步获取最新差异".to_owned()),
        }
    }

    pub async fn commit(
        &self,
        prepared: Arc<Prepared>,
        resolutions: &[CloudSyncResolution],
    ) -> Result<TransferStats, String> {
        let mut merged = merge::merge(
            &prepared.baseline,
            &prepared.local,
            &prepared.remote,
            &prepared.manifest,
            resolutions,
        )?;
        if merged.unresolved {
            return Err("请先处理所有同步冲突".to_owned());
        }
        normalize_order(&mut merged.documents)?;
        let validation_replica = Arc::clone(&self.replica);
        let validation_documents = merged.documents.clone();
        tauri::async_runtime::spawn_blocking(move || {
            validation_replica.validate(&validation_documents)
        })
        .await
        .map_err(|_| "校验同步数据的后台任务异常")??;
        self.verify_fresh(&prepared).await?;
        let mut entries = BTreeMap::new();
        let mut uploads = Vec::new();
        for (key, document) in &merged.documents {
            let hash = document.hash()?;
            if let Some(entry) = prepared
                .manifest
                .entries
                .get(key)
                .filter(|entry| entry.object.is_some() && entry.hash == hash)
            {
                entries.insert(key.clone(), entry.clone());
            } else {
                uploads.push((key.clone(), document.clone(), hash));
            }
        }
        for (key, old) in prepared
            .baseline
            .entries
            .iter()
            .chain(&prepared.manifest.entries)
        {
            if !key.starts_with("blob/") && !merged.documents.contains_key(key) {
                entries.insert(
                    key.clone(),
                    Entry::deleted(old.title.clone(), old.category.clone()),
                );
            }
        }
        let mut stats = prepared.stats.clone();
        stats.total = uploads.len();
        stats.done = 0;
        let mut futures = stream::iter(uploads)
            .map(|(key, document, hash)| {
                let cipher = Arc::clone(&prepared.cipher);
                async move {
                    self.control.check()?;
                    let plain = serde_json::to_vec(&document).map_err(|_| "无法编码同步条目")?;
                    if plain.len() > MAX_OBJECT_BYTES - 40 {
                        return Err("单项同步数据过大，请缩小资源后重试".to_owned());
                    }
                    let bytes = cipher.encrypt(&plain, b"balancehub-record-v1")?;
                    let object = format::digest(&bytes);
                    let count = bytes.len() as u64;
                    files::write_bytes(&self.root.join("cache").join(&object), &bytes)?;
                    let created = self
                        .dav
                        .put(&format!("objects/{object}"), bytes, None)
                        .await?;
                    if !created {
                        match self
                            .dav
                            .get(&format!("objects/{object}"), None, MAX_OBJECT_BYTES)
                            .await?
                        {
                            Download::Found { bytes, .. } if format::digest(&bytes) == object => {}
                            _ => return Err("云端资源发生冲突，未提交新快照".to_owned()),
                        }
                    }
                    Ok::<_, String>((
                        key,
                        Entry {
                            hash,
                            object: Some(object),
                            title: document.title,
                            category: document.category,
                        },
                        if created { count } else { 0 },
                    ))
                }
            })
            .buffer_unordered(4);
        while let Some((key, entry, count)) = futures.try_next().await? {
            entries.insert(key, entry);
            stats.done += 1;
            if count > 0 {
                stats.uploaded += 1;
                stats.bytes += count;
            }
            (self.progress)(
                CloudSyncPhase::Uploading,
                "正在上传变化的配置与资源",
                stats.clone(),
            );
        }
        drop(futures);
        self.verify_fresh(&prepared).await?;
        self.control.check()?;
        self.control.begin_commit()?;
        let changed = entries != prepared.manifest.entries || prepared.head.is_none();
        let (head, etag) = if changed {
            let manifest = Manifest {
                version: VERSION,
                device: self.device.clone(),
                updated_at: format::now(),
                entries: entries.clone(),
            };
            let bytes = prepared.cipher.encode_head(&manifest)?;
            if bytes.len() > MAX_HEAD_BYTES {
                return Err("同步清单过大，未提交远端变更".to_owned());
            }
            let digest = format::digest(&bytes);
            let head = parse_head(&bytes)?;
            (self.progress)(
                CloudSyncPhase::Publishing,
                "正在提交完整同步版本",
                stats.clone(),
            );
            if !self
                .dav
                .put("head.json", bytes, prepared.etag.as_deref())
                .await?
            {
                return Err("其他设备刚刚更新了云端，已保留双方数据；请重新同步".to_owned());
            }
            let etag = match self.dav.get("head.json", None, MAX_HEAD_BYTES).await? {
                Download::Found { bytes, etag } if format::digest(&bytes) == digest => Some(etag),
                _ => None,
            };
            (Some(head), etag)
        } else {
            (prepared.head.clone(), prepared.etag.clone())
        };
        (self.progress)(
            CloudSyncPhase::Applying,
            "正在应用配置并保存恢复点",
            stats.clone(),
        );
        let replica = Arc::clone(&self.replica);
        let expected = prepared.local.clone();
        let desired = merged.documents;
        tauri::async_runtime::spawn_blocking(move || replica.apply(&expected, &desired))
            .await
            .map_err(|_| "本地应用同步结果的后台任务异常")??;
        files::write_json(
            &self.root.join("baseline.json"),
            &Baseline {
                initialized: true,
                entries: entries.clone(),
                head,
                etag,
            },
        )?;
        prune_cache(&self.root, &entries);
        Ok(stats)
    }
}

fn prune_cache(root: &std::path::Path, entries: &BTreeMap<String, Entry>) {
    let keep: BTreeSet<_> = entries
        .values()
        .filter_map(|entry| entry.object.as_deref())
        .collect();
    if let Ok(files) = std::fs::read_dir(root.join("cache")) {
        for entry in files.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_file()) {
                if let Some(name) = entry
                    .file_name()
                    .to_str()
                    .filter(|name| format::valid_object_id(name))
                {
                    if !keep.contains(name) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
}

fn valid_key(key: &str) -> bool {
    key.len() <= 512
        && !key.chars().any(char::is_control)
        && (key == "order/providers"
            || ["provider/", "asset/", "setting/", "blob/"]
                .iter()
                .any(|prefix| {
                    key.strip_prefix(prefix)
                        .is_some_and(|id| !id.is_empty() && !id.contains('/'))
                }))
}

fn normalize_order(documents: &mut SyncDocuments) -> Result<(), String> {
    let ids: BTreeSet<_> = documents
        .keys()
        .filter_map(|key| key.strip_prefix("provider/").map(str::to_owned))
        .collect();
    if ids.is_empty() {
        documents.remove("order/providers");
        return Ok(());
    }
    let mut order: Vec<String> = documents
        .get("order/providers")
        .map(|doc| serde_json::from_value(doc.value.clone()))
        .transpose()
        .map_err(|_| "中转站排序格式无效")?
        .unwrap_or_default();
    let mut seen = BTreeSet::new();
    order.retain(|id| ids.contains(id) && seen.insert(id.clone()));
    order.extend(ids.into_iter().filter(|id| !seen.contains(id)));
    documents.insert(
        "order/providers".to_owned(),
        SyncDocument {
            title: "中转站排序".to_owned(),
            category: "偏好".to_owned(),
            value: serde_json::json!(order),
        },
    );
    Ok(())
}
