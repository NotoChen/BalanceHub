use super::{
    config::Settings,
    core::{Control, Engine, Replica, TransferStats},
    crypto::{parse_head, Cipher},
    files,
    format::{
        self, Baseline, Entry, Manifest, SyncDocument, SyncDocuments, MAX_HEAD_BYTES, VERSION,
    },
    merge, projection,
    transport::{DavClient, Download},
};
use crate::models::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

const PASSPHRASE: &str = "fixture-sync-passphrase-2026";

fn document(title: &str, value: serde_json::Value) -> SyncDocument {
    SyncDocument {
        title: title.to_owned(),
        category: "偏好".to_owned(),
        value,
    }
}

fn baseline(documents: &SyncDocuments) -> Baseline {
    Baseline {
        initialized: true,
        entries: documents
            .iter()
            .map(|(key, document)| {
                (
                    key.clone(),
                    Entry {
                        hash: document.hash().unwrap(),
                        object: Some("a".repeat(64)),
                        title: document.title.clone(),
                        category: document.category.clone(),
                    },
                )
            })
            .collect(),
        ..Baseline::default()
    }
}

fn manifest(documents: &SyncDocuments) -> Manifest {
    Manifest {
        version: VERSION,
        entries: baseline(documents).entries,
        ..Manifest::default()
    }
}

#[test]
fn encryption_rejects_wrong_password_corruption_and_record_manifest_swaps() {
    let cipher = Cipher::derive(PASSPHRASE, None).unwrap();
    let original = b"fixture secret must not be visible";
    let encoded = cipher.encrypt(original, b"balancehub-record-v1").unwrap();
    assert!(!encoded
        .windows(original.len())
        .any(|bytes| bytes == original));
    assert_eq!(
        cipher.decrypt(&encoded, b"balancehub-record-v1").unwrap(),
        original
    );
    assert!(cipher.decrypt(&encoded, b"balancehub-manifest-v1").is_err());
    let mut corrupted = encoded.clone();
    corrupted[32] ^= 1;
    assert!(cipher.decrypt(&corrupted, b"balancehub-record-v1").is_err());
    let wrong = Cipher::derive("different-fixture-passphrase", Some(&cipher.salt)).unwrap();
    assert!(wrong.decrypt(&encoded, b"balancehub-record-v1").is_err());
    let head = cipher
        .encode_head(&Manifest {
            version: VERSION,
            ..Manifest::default()
        })
        .unwrap();
    assert_eq!(
        cipher
            .decode_head(&parse_head(&head).unwrap())
            .unwrap()
            .version,
        VERSION
    );
}

#[test]
fn three_way_merge_propagates_deletion_and_requires_delete_edit_resolution() {
    let old = BTreeMap::from([(
        "setting/themeMode".to_owned(),
        document("主题", json!("dark")),
    )]);
    let base = baseline(&old);
    let mut remote = manifest(&old);
    remote.entries.insert(
        "setting/themeMode".to_owned(),
        Entry::deleted("主题".to_owned(), "偏好".to_owned()),
    );
    let merged = merge::merge(&base, &old, &BTreeMap::new(), &remote, &[]).unwrap();
    assert!(!merged.unresolved);
    assert!(merged.documents.is_empty());
    let edited = BTreeMap::from([(
        "setting/themeMode".to_owned(),
        document("主题", json!("light")),
    )]);
    assert!(
        merge::merge(&base, &edited, &BTreeMap::new(), &remote, &[])
            .unwrap()
            .unresolved
    );
    let resolution = [CloudSyncResolution {
        key: "setting/themeMode".to_owned(),
        side: CloudSyncSide::Remote,
    }];
    assert!(
        merge::merge(&base, &edited, &BTreeMap::new(), &remote, &resolution)
            .unwrap()
            .documents
            .is_empty()
    );
    assert!(
        merge::merge(&Baseline::default(), &old, &BTreeMap::new(), &remote, &[])
            .unwrap()
            .unresolved
    );
}

#[test]
fn independent_provider_additions_do_not_create_an_order_conflict() {
    let base = BTreeMap::from([(
        "order/providers".to_owned(),
        document("中转站排序", json!(["a", "b"])),
    )]);
    let local = BTreeMap::from([(
        "order/providers".to_owned(),
        document("中转站排序", json!(["a", "new-local", "b"])),
    )]);
    let remote = BTreeMap::from([(
        "order/providers".to_owned(),
        document("中转站排序", json!(["a", "new-remote", "b"])),
    )]);
    let merged = merge::merge(&baseline(&base), &local, &remote, &manifest(&remote), &[]).unwrap();
    assert!(!merged.unresolved);
    assert_eq!(
        merged.documents["order/providers"].value,
        json!(["a", "new-local", "new-remote", "b"])
    );
    let reordered = BTreeMap::from([(
        "order/providers".to_owned(),
        document("中转站排序", json!(["b", "a"])),
    )]);
    assert!(
        merge::merge(
            &baseline(&base),
            &local,
            &reordered,
            &manifest(&reordered),
            &[]
        )
        .unwrap()
        .unresolved
    );
}

#[test]
fn portable_projection_retains_local_sessions_and_excludes_automatic_execution_and_caches() {
    let mut input = ProviderInput::default();
    input.identity.name = "Fixture relay".to_owned();
    input.identity.base_url = "https://fixture.invalid".to_owned();
    input.identity.protocol = ProviderProtocol::Sub2Api;
    input.auth.login_username = "fixture".to_owned();
    input.auth.login_password = "fixture-account-password".to_owned();
    input.auth.api_key = "sk-fixture-key".to_owned();
    let mut provider = Provider::from_input(input, "fixture-relay".to_owned());
    provider.auth.access_token = "fixture-short-lived-token-a".to_owned();
    provider.auth.refresh_token = "fixture-refresh-token-a".to_owned();
    provider.auth.access_token_expires_at = Some(999999);
    provider.quota.available = 42.0;
    let mut source = AppData::new_current(vec![provider], AppSettings::default());
    source.settings.proxy_url = "http://fixture-proxy.invalid:8888".to_owned();
    source.settings.theme_mode = ThemeMode::Dark;
    let documents = projection::app_documents(&source).unwrap();
    let text = serde_json::to_string(&documents).unwrap();
    for excluded in [
        "short-lived",
        "refresh-token",
        "fixture-proxy",
        "quota",
        "workspaces",
        "browserBinding",
    ] {
        assert!(!text.contains(excluded), "{excluded}");
    }
    let imported = projection::apply_app_documents(&AppData::default(), &documents).unwrap();
    assert_eq!(projection::app_documents(&imported).unwrap(), documents);
    assert!(imported.providers[0].auth.access_token.is_empty());
    assert!(!imported.providers[0].liveness.enabled);
    assert!(!imported.providers[0].quota.known);
    let mut current = source.clone();
    current.providers[0].auth.access_token = "fixture-local-token-b".to_owned();
    current.providers[0].auth.refresh_token = "fixture-local-refresh-b".to_owned();
    let mut remote = source.clone();
    remote.providers[0].auth.api_key_options[0].local_name = "Remote key remark".to_owned();
    let applied =
        projection::apply_app_documents(&current, &projection::app_documents(&remote).unwrap())
            .unwrap();
    assert_eq!(
        applied.providers[0].auth.access_token,
        "fixture-local-token-b"
    );
    assert_eq!(
        applied.providers[0].auth.refresh_token,
        "fixture-local-refresh-b"
    );
    assert_eq!(applied.settings.proxy_url, source.settings.proxy_url);
    current.providers[0].quota.available = 100.0;
    assert!(!projection::app_changed(&source, &current));
}

#[test]
fn settings_secrets_are_preserved_only_for_the_same_account_and_are_never_in_views() {
    let saved = Settings {
        server_url: "https://dav.fixture.invalid/".to_owned(),
        username: "fixture".to_owned(),
        password: "private-fixture".to_owned(),
        passphrase: PASSPHRASE.to_owned(),
        ..Settings::default()
    };
    let input = CloudSyncSettingsInput {
        server_url: saved.server_url.clone(),
        username: saved.username.clone(),
        remote_root: saved.remote_root.clone(),
        device_name: "Device A".to_owned(),
        auto_sync: true,
        ..CloudSyncSettingsInput::default()
    };
    let updated = saved.update(input.clone()).unwrap();
    assert_eq!(updated.password, saved.password);
    assert_eq!(updated.passphrase, saved.passphrase);
    let public = serde_json::to_string(&updated.view()).unwrap();
    assert!(!public.contains(PASSPHRASE));
    assert!(!public.contains("private-fixture"));
    assert!(saved
        .update(CloudSyncSettingsInput {
            server_url: "https://different.fixture.invalid/".to_owned(),
            ..input
        })
        .is_err());
    let root = tempfile::tempdir().unwrap();
    updated.save(root.path()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(root.path().join("settings.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn cancellation_and_commit_are_mutually_exclusive() {
    let cancelled = Control::default();
    assert!(cancelled.cancel());
    assert!(cancelled.begin_commit().is_err());
    let committing = Control::default();
    committing.begin_commit().unwrap();
    assert!(!committing.cancel());
    assert!(!committing.expire());
    assert!(!committing.can_cancel());
}

#[derive(Default)]
struct MemoryReplica {
    documents: Mutex<SyncDocuments>,
    fail_apply: AtomicBool,
}
impl Replica for MemoryReplica {
    fn snapshot(&self) -> Result<SyncDocuments, String> {
        Ok(self.documents.lock().unwrap().clone())
    }
    fn validate(&self, _documents: &SyncDocuments) -> Result<(), String> {
        Ok(())
    }
    fn apply(&self, expected: &SyncDocuments, desired: &SyncDocuments) -> Result<(), String> {
        if self.fail_apply.swap(false, Ordering::AcqRel) {
            return Err("fixture local apply failure".to_owned());
        }
        let mut current = self.documents.lock().unwrap();
        if &*current != expected {
            return Err("fixture local revision changed".to_owned());
        }
        *current = desired.clone();
        Ok(())
    }
    fn recovery(&self) -> Result<Option<SyncDocuments>, String> {
        Ok(None)
    }
}

struct Device {
    root: tempfile::TempDir,
    replica: Arc<MemoryReplica>,
    name: String,
    space: String,
}
impl Device {
    fn new(name: &str, space: &str, documents: SyncDocuments) -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
            replica: Arc::new(MemoryReplica {
                documents: Mutex::new(documents),
                ..MemoryReplica::default()
            }),
            name: name.to_owned(),
            space: space.to_owned(),
        }
    }
    fn engine(&self) -> Engine {
        let preferences = AppSettings {
            proxy_mode: ProxyMode::NoProxy,
            ..AppSettings::default()
        };
        Engine {
            root: self.root.path().to_path_buf(),
            dav: DavClient::new(
                crate::network::build_sync_client(&preferences).unwrap(),
                &test_url(),
                &self.space,
                "balancehub-fixture",
                "balancehub-fixture-password",
            )
            .unwrap(),
            passphrase: PASSPHRASE.to_owned(),
            device: self.name.clone(),
            replica: self.replica.clone(),
            progress: Arc::new(|_, _, _| {}),
            control: Arc::default(),
        }
    }
    async fn sync(&self, resolutions: &[CloudSyncResolution]) -> Result<TransferStats, String> {
        let engine = self.engine();
        let prepared = Arc::new(engine.prepare().await?);
        engine.commit(prepared, resolutions).await
    }
    fn set(&self, key: &str, title: &str, value: serde_json::Value) {
        self.replica
            .documents
            .lock()
            .unwrap()
            .insert(key.to_owned(), document(title, value));
    }
    fn documents(&self) -> SyncDocuments {
        self.replica.snapshot().unwrap()
    }
}

fn test_url() -> String {
    std::env::var("BALANCEHUB_WEBDAV_TEST_URL")
        .expect("start scripts/webdav-test-server.py and set BALANCEHUB_WEBDAV_TEST_URL")
}
fn space() -> String {
    format!("suite-{}", format::random_id().unwrap())
}
async fn fault(space: &str, mode: Option<&str>, after: usize) {
    let base = reqwest::Url::parse(&test_url()).unwrap();
    let prefix = format!("{}{space}/", base.path());
    let url = base.join("/__balancehub_test__/fault").unwrap();
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(url)
        .json(&json!({"prefix": prefix, "mode": mode, "after": after}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
}

#[tokio::test]
#[ignore = "requires the local WsgiDAV fixture"]
async fn real_dav_two_devices_merge_restart_deletion_and_stale_preview() {
    let space = space();
    let a = Device::new("Device A", &space, BTreeMap::new());
    a.set("setting/themeMode", "主题", json!("dark"));
    let engine = a.engine();
    engine.dav.test().await.unwrap();
    let prepared = Arc::new(engine.prepare().await.unwrap());
    assert!(prepared.review().unwrap().initial);
    assert_eq!(prepared.review().unwrap().changes.len(), 1);
    let first = engine.commit(prepared, &[]).await.unwrap();
    assert_eq!(first.uploaded, 1);
    let b = Device::new("Device B", &space, BTreeMap::new());
    assert_eq!(b.sync(&[]).await.unwrap().downloaded, 1);
    assert_eq!(a.documents(), b.documents());
    // New engine instance, same persisted common ancestor and encrypted cache.
    let unchanged = b.sync(&[]).await.unwrap();
    assert_eq!(unchanged.uploaded + unchanged.downloaded, 0);
    assert!(b.engine().remote_unchanged().await.unwrap());
    a.set(
        "setting/livenessModel",
        "测活模型",
        json!("fixture-model-a"),
    );
    b.set("setting/refreshInterval", "刷新间隔", json!(300));
    a.sync(&[]).await.unwrap();
    b.sync(&[]).await.unwrap();
    a.sync(&[]).await.unwrap();
    assert_eq!(a.documents(), b.documents());
    a.set("setting/themeMode", "主题", json!("light"));
    b.set("setting/themeMode", "主题", json!("system"));
    a.sync(&[]).await.unwrap();
    let engine = b.engine();
    let conflict = Arc::new(engine.prepare().await.unwrap());
    assert_eq!(
        conflict
            .review()
            .unwrap()
            .changes
            .iter()
            .filter(|change| change.conflict)
            .count(),
        1
    );
    assert!(engine.commit(Arc::clone(&conflict), &[]).await.is_err());
    engine
        .commit(
            conflict,
            &[CloudSyncResolution {
                key: "setting/themeMode".to_owned(),
                side: CloudSyncSide::Remote,
            }],
        )
        .await
        .unwrap();
    assert_eq!(b.documents()["setting/themeMode"].value, json!("light"));
    // A removed record stays removed when an offline device comes back.
    a.replica
        .documents
        .lock()
        .unwrap()
        .remove("setting/livenessModel");
    a.sync(&[]).await.unwrap();
    b.sync(&[]).await.unwrap();
    assert!(!b.documents().contains_key("setting/livenessModel"));
    let engine = b.engine();
    let stale = Arc::new(engine.prepare().await.unwrap());
    a.set("setting/refreshInterval", "刷新间隔", json!(600));
    a.sync(&[]).await.unwrap();
    assert!(engine
        .commit(stale, &[])
        .await
        .unwrap_err()
        .contains("云端配置"));
    b.sync(&[]).await.unwrap();
    let engine = b.engine();
    let stale = Arc::new(engine.prepare().await.unwrap());
    b.set("setting/refreshInterval", "刷新间隔", json!(900));
    assert!(engine
        .commit(stale, &[])
        .await
        .unwrap_err()
        .contains("本地配置"));
}

fn skill_documents(text: &str) -> SyncDocuments {
    let manifest =
        b"---\nname: fixture-skill\ndescription: Isolated WebDAV fixture\n---\n# Fixture\n";
    let blob1 = format!("blob/{}", format::digest(manifest));
    let blob2 = format!("blob/{}", format::digest(text.as_bytes()));
    let attachment = |bytes: &[u8]| SyncDocument {
        title: "资源文件".to_owned(),
        category: "blob".to_owned(),
        value: json!({"content": STANDARD.encode(bytes)}),
    };
    BTreeMap::from([
        (
            "asset/fixture-skill".to_owned(),
            SyncDocument {
                title: "fixture-skill".to_owned(),
                category: "共享资产".to_owned(),
                value: json!({"name":"fixture-skill", "category":"skill", "files":{
                    "SKILL.md": {"blob":blob1, "executable":false}, "scripts/example.sh":{"blob":blob2, "executable":true}
                }}),
            },
        ),
        (blob1, attachment(manifest)),
        (blob2, attachment(text.as_bytes())),
    ])
}

fn mcp_hook_documents() -> SyncDocuments {
    let mut documents = BTreeMap::new();
    for (id, transport, command, url) in [
        ("stdio", "stdio", Some("fixture-mcp"), None),
        (
            "http",
            "http",
            None,
            Some("https://mcp.fixture.invalid/mcp"),
        ),
        ("sse", "sse", None, Some("https://mcp.fixture.invalid/sse")),
        (
            "websocket",
            "webSocket",
            None,
            Some("wss://mcp.fixture.invalid/ws"),
        ),
    ] {
        let name = format!("fixture-mcp-{id}");
        documents.insert(format!("asset/{name}"), SyncDocument { title: name.clone(), category: "共享资产".to_owned(), value: json!({"name":name,"category":"mcp","definition":{"Mcp":{
            "transport":transport,"command":command,"args":[],"url":url,"cwd":null,"environment":{},"headers":{},"native_options":{},"native_adapter":null
        }}}) });
    }
    documents.insert("asset/fixture-hook".to_owned(), SyncDocument { title: "Fixture hook".to_owned(), category: "共享资产".to_owned(), value: json!({"name":"Fixture hook","category":"hook","definition":{"Hook":{(AgentCliKind::ClaudeCode.key()):{
        "event":"PreToolUse","group":{"matcher":"Bash","hooks":[{"type":"command","command":"printf fixture-hook"}]}
    }}}}) });
    documents
}

#[test]
fn mcp_protocols_and_hook_shared_definitions_round_trip_without_native_application() {
    let root = tempfile::tempdir().unwrap();
    let service = crate::services::agent_cli::catalog::CatalogService::new(
        root.path().canonicalize().unwrap().join("library"),
        Arc::default(),
    );
    let documents = mcp_hook_documents();
    service.validate_cloud_snapshot(&documents).unwrap();
    service
        .apply_cloud_snapshot(&BTreeMap::new(), &documents, |_, _, write| write())
        .unwrap();
    assert_eq!(service.cloud_snapshot().unwrap(), documents);
    let mut invalid = documents.clone();
    invalid.get_mut("asset/fixture-mcp-http").unwrap().value["definition"]["Mcp"]["url"] =
        json!("file:///fixture");
    assert!(service.validate_cloud_snapshot(&invalid).is_err());
    assert_eq!(service.cloud_snapshot().unwrap(), documents);
}

#[test]
#[ignore = "generates a temporary isolated desktop acceptance fixture"]
fn write_desktop_fixture() {
    let Ok(path) = std::env::var("BALANCEHUB_WEBDAV_FIXTURE_DIR") else {
        return;
    };
    let root = std::path::PathBuf::from(path).canonicalize().unwrap();
    assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
    let providers = [
        ("fixture-ui-a", "演示中转站 A", ProviderProtocol::NewApi),
        ("fixture-ui-b", "演示网关 B", ProviderProtocol::Api),
    ]
    .into_iter()
    .map(|(id, name, protocol)| {
        let mut input = ProviderInput::default();
        input.identity.name = name.to_owned();
        input.identity.base_url = format!("https://{id}.invalid");
        input.identity.protocol = protocol;
        input.auth.api_key = format!("sk-{id}-synthetic-key");
        if protocol == ProviderProtocol::NewApi {
            input.auth.login_username = "fixture-user".to_owned();
            input.auth.login_password = "synthetic-password".to_owned();
        }
        Provider::from_input(input, id.to_owned())
    })
    .collect();
    let settings = AppSettings {
        onboarding_completed: true,
        auto_refresh_enabled: false,
        auto_check_in_enabled: false,
        liveness_enabled: false,
        notification_enabled: false,
        session_index_enabled: false,
        theme_mode: ThemeMode::Light,
        proxy_mode: ProxyMode::NoProxy,
        ..AppSettings::default()
    };
    files::write_json(
        &root.join("data.json"),
        &AppData::new_current(providers, settings),
    )
    .unwrap();
    let catalog = crate::services::agent_cli::catalog::CatalogService::new(
        root.join("agent-asset-library"),
        Arc::default(),
    );
    let mut documents = skill_documents("printf fixture-desktop\n");
    documents.extend(mcp_hook_documents());
    catalog
        .apply_cloud_snapshot(&BTreeMap::new(), &documents, |_, _, write| write())
        .unwrap();
}

#[tokio::test]
#[ignore = "requires the local WsgiDAV fixture"]
async fn real_dav_skill_file_incremental_encryption_and_diff() {
    let space = space();
    let a = Device::new("Device A", &space, skill_documents("printf fixture-one\n"));
    a.engine().dav.test().await.unwrap();
    assert_eq!(a.sync(&[]).await.unwrap().uploaded, 3);
    let b = Device::new("Device B", &space, BTreeMap::new());
    assert_eq!(b.sync(&[]).await.unwrap().downloaded, 3);
    *a.replica.documents.lock().unwrap() = skill_documents("printf fixture-two\n");
    assert_eq!(a.sync(&[]).await.unwrap().uploaded, 2);
    let engine = b.engine();
    let prepared = Arc::new(engine.prepare().await.unwrap());
    assert_eq!(prepared.stats.downloaded, 2);
    let diff = super::comparison::compare(&prepared, "asset/fixture-skill").unwrap();
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].path, "scripts/example.sh");
    assert_eq!(diff.files[0].local_text, "printf fixture-one\n");
    assert_eq!(diff.files[0].remote_text, "printf fixture-two\n");
    engine.commit(prepared, &[]).await.unwrap();
    assert_eq!(a.documents(), b.documents());
    let Download::Found { bytes, .. } = a
        .engine()
        .dav
        .get("head.json", None, MAX_HEAD_BYTES)
        .await
        .unwrap()
    else {
        panic!("missing head");
    };
    assert!(!String::from_utf8_lossy(&bytes).contains("fixture-skill"));
    let baseline: Baseline = files::read_json(&a.root.path().join("baseline.json"))
        .unwrap()
        .unwrap();
    for entry in baseline.entries.values() {
        if let Some(id) = &entry.object {
            let Download::Found { bytes, .. } = a
                .engine()
                .dav
                .get(&format!("objects/{id}"), None, 1024 * 1024)
                .await
                .unwrap()
            else {
                panic!("missing object");
            };
            assert!(!String::from_utf8_lossy(&bytes).contains("fixture-two"));
        }
    }
}

#[tokio::test]
#[ignore = "requires the local WsgiDAV fixture"]
async fn real_dav_faults_keep_valid_head_and_local_data_recoverable() {
    let space = space();
    let a = Device::new("Device A", &space, BTreeMap::new());
    a.set("setting/themeMode", "主题", json!("dark"));
    a.engine().dav.test().await.unwrap();
    a.sync(&[]).await.unwrap();
    let b = Device::new("Device B", &space, BTreeMap::new());
    b.sync(&[]).await.unwrap();
    let before = b.documents();
    a.set(
        "setting/livenessModel",
        "测活模型",
        json!("fixture-change-a"),
    );
    a.set("setting/refreshInterval", "刷新间隔", json!(900));
    fault(&space, Some("object_failure"), 1).await;
    assert!(a.sync(&[]).await.is_err());
    fault(&space, None, 0).await;
    b.sync(&[]).await.unwrap();
    assert_eq!(b.documents(), before);
    a.sync(&[]).await.unwrap();
    for mode in ["tamper", "missing_object", "offline"] {
        let fresh = Device::new("Fresh", &space, BTreeMap::new());
        fault(&space, Some(mode), 0).await;
        assert!(fresh.sync(&[]).await.is_err());
        assert!(fresh.documents().is_empty());
        fault(&space, None, 0).await;
    }
    let wrong = Device::new("Wrong password", &space, BTreeMap::new());
    let mut engine = wrong.engine();
    engine.passphrase = "wrong-fixture-password".to_owned();
    assert!(engine.prepare().await.err().unwrap().contains("解密失败"));
    assert!(wrong.documents().is_empty());
    // Remote commit succeeds, but the HTTP acknowledgement is lost.
    a.set("setting/refreshInterval", "刷新间隔", json!(1200));
    fault(&space, Some("lost_head_ack"), 0).await;
    assert!(a.sync(&[]).await.is_err());
    fault(&space, None, 0).await;
    a.sync(&[]).await.unwrap();
    b.sync(&[]).await.unwrap();
    assert_eq!(a.documents(), b.documents());
    // Local persistence failure after downloading can be retried unchanged.
    a.set("setting/themeMode", "主题", json!("light"));
    a.sync(&[]).await.unwrap();
    let before = b.documents();
    b.replica.fail_apply.store(true, Ordering::Release);
    assert!(b.sync(&[]).await.is_err());
    assert_eq!(b.documents(), before);
    b.sync(&[]).await.unwrap();
    assert_eq!(a.documents(), b.documents());
}

#[tokio::test]
#[ignore = "requires the local WsgiDAV fixture"]
async fn real_dav_rejects_ignored_conditions_and_honors_cas_and_unicode_directory() {
    let unsafe_space = space();
    let unsafe_device = Device::new("Fixture", &unsafe_space, BTreeMap::new());
    fault(&unsafe_space, Some("ignore_conditions"), 0).await;
    assert!(unsafe_device
        .engine()
        .dav
        .test()
        .await
        .unwrap_err()
        .contains("忽略条件写入"));
    fault(&unsafe_space, None, 0).await;
    fault(&unsafe_space, Some("coarse_etag"), 0).await;
    assert!(unsafe_device
        .engine()
        .dav
        .test()
        .await
        .unwrap_err()
        .contains("版本标识"));
    fault(&unsafe_space, None, 0).await;
    let directory = format!("{}/中文目录 with space/设备", space());
    let device = Device::new("Fixture", &directory, BTreeMap::new());
    let engine = device.engine();
    engine.dav.test().await.unwrap();
    assert!(engine
        .dav
        .put("cas-test", b"first".to_vec(), None)
        .await
        .unwrap());
    let Download::Found { etag, .. } = engine.dav.get("cas-test", None, 1024).await.unwrap() else {
        panic!("missing CAS fixture");
    };
    assert!(matches!(
        engine.dav.get("cas-test", Some(&etag), 1024).await.unwrap(),
        Download::Unchanged
    ));
    assert!(engine
        .dav
        .put("cas-test", b"second-version".to_vec(), Some(&etag))
        .await
        .unwrap());
    assert!(!engine
        .dav
        .put("cas-test", b"lost-update".to_vec(), Some(&etag))
        .await
        .unwrap());
}

#[test]
fn shared_skill_projection_reuses_library_rules_without_native_configuration_writes() {
    let root = tempfile::tempdir().unwrap();
    let service = crate::services::agent_cli::catalog::CatalogService::new(
        root.path().canonicalize().unwrap().join("library"),
        Arc::default(),
    );
    let documents = skill_documents("printf fixture\n");
    service.validate_cloud_snapshot(&documents).unwrap();
    assert!(!root.path().join("library/library.json").exists());
    service
        .apply_cloud_snapshot(&BTreeMap::new(), &documents, |before, after, write| {
            assert!(before.is_none());
            assert!(after.is_some());
            write()
        })
        .unwrap();
    assert_eq!(service.cloud_snapshot().unwrap(), documents);
    service
        .apply_cloud_snapshot(&documents, &BTreeMap::new(), |before, after, write| {
            assert!(before.is_some());
            assert!(after.is_some());
            write()
        })
        .unwrap();
    assert!(service.cloud_snapshot().unwrap().is_empty());
    let history: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("library/library.json")).unwrap())
            .unwrap();
    assert_eq!(
        history["entries"]["fixture-skill"]["versions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(history["entries"]["fixture-skill"]["shared_deleted"], true);
}
