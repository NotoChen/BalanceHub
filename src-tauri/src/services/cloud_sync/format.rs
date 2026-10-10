use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(super) const FORMAT: &str = "balancehub-sync";
pub(super) const VERSION: u32 = 1;
pub(super) const MAX_HEAD_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_OBJECT_BYTES: usize = 96 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SyncDocument {
    pub title: String,
    pub category: String,
    pub value: Value,
}

pub(crate) type SyncDocuments = BTreeMap<String, SyncDocument>;

impl SyncDocument {
    pub(super) fn hash(&self) -> Result<String, String> {
        serde_json::to_vec(self)
            .map(|bytes| digest(&bytes))
            .map_err(|_| "无法序列化同步数据".to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Entry {
    pub hash: String,
    /// None is a retained deletion marker, never an absent/unknown record.
    pub object: Option<String>,
    pub title: String,
    pub category: String,
}

impl Entry {
    pub(super) fn deleted(title: String, category: String) -> Self {
        Self {
            hash: String::new(),
            object: None,
            title,
            category,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Manifest {
    pub version: u32,
    pub device: String,
    pub updated_at: u64,
    pub entries: BTreeMap<String, Entry>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Head {
    pub format: String,
    pub version: u32,
    pub salt: String,
    pub manifest: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Baseline {
    pub initialized: bool,
    pub entries: BTreeMap<String, Entry>,
    pub etag: Option<String>,
    pub head: Option<Head>,
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn random_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "无法生成同步标识".to_owned())?;
    Ok(format!("{:032x}", u128::from_ne_bytes(bytes)))
}

pub(super) fn now() -> u64 {
    crate::util::unix_millis() as u64
}

pub(super) fn valid_object_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
