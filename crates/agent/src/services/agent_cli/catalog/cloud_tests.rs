//! 共享库与 WebDAV 同步的衔接测试。
//!
//! 同步引擎在 `balancehub-core`，这里验证桌面资产库对同步文档的读写：
//! 共享定义只落在 BalanceHub 自己的库里，不会直接写入各 Agent 的原生配置。

use balancehub_core::services::cloud_sync::{SyncDocument, SyncDocuments};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::{collections::BTreeMap, sync::Arc};

use super::CatalogService;

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn skill_documents(text: &str) -> SyncDocuments {
    let manifest =
        b"---\nname: fixture-skill\ndescription: Isolated WebDAV fixture\n---\n# Fixture\n";
    let blob1 = format!("blob/{}", digest(manifest));
    let blob2 = format!("blob/{}", digest(text.as_bytes()));
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
    documents.insert("asset/fixture-hook".to_owned(), SyncDocument { title: "Fixture hook".to_owned(), category: "共享资产".to_owned(), value: json!({"name":"Fixture hook","category":"hook","definition":{"Hook":{(crate::models::AgentCliKind::ClaudeCode.key()):{
        "event":"PreToolUse","group":{"matcher":"Bash","hooks":[{"type":"command","command":"printf fixture-hook"}]}
    }}}}) });
    documents
}

#[test]
fn mcp_protocols_and_hook_shared_definitions_round_trip_without_native_application() {
    let root = tempfile::tempdir().unwrap();
    let service = CatalogService::new(
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
fn shared_skill_projection_reuses_library_rules_without_native_configuration_writes() {
    let root = tempfile::tempdir().unwrap();
    let service = CatalogService::new(
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
        history["definitions"]["fixture-skill"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        history["library"]["entries"]["fixture-skill"]["shared_deleted"],
        true
    );
}
