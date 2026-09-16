use serde::Deserialize;
use std::{collections::HashMap, sync::OnceLock};

#[derive(Debug, Clone, Deserialize)]
pub(super) struct Artifact {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Target {
    pub node: Artifact,
    pub node_prefix: String,
    pub browser: Artifact,
    pub browser_executable: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Manifest {
    pub version: String,
    pub browser_version: String,
    pub playwright: Artifact,
    targets: HashMap<String, Target>,
}

pub(super) fn manifest() -> &'static Manifest {
    static MANIFEST: OnceLock<Manifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../../../browser-worker/runtime-manifest.json"
        ))
        .expect("bundled browser manifest")
    })
}

pub(super) fn target() -> Result<&'static Target, String> {
    let key = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    manifest()
        .targets
        .get(&key)
        .ok_or_else(|| "当前系统架构暂不支持浏览器签到组件".to_string())
}

pub(super) const WORKER: &str = include_str!("../../../../browser-worker/worker.mjs");
pub(super) fn node_name() -> &'static str {
    if cfg!(windows) {
        "node.exe"
    } else {
        "node"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_artifacts_are_pinned_to_official_https_downloads() {
        let manifest = manifest();
        assert_eq!(manifest.targets.len(), 6);
        for artifact in std::iter::once(&manifest.playwright).chain(
            manifest
                .targets
                .values()
                .flat_map(|target| [&target.node, &target.browser]),
        ) {
            let url = reqwest::Url::parse(&artifact.url).unwrap();
            assert_eq!(url.scheme(), "https");
            assert!(matches!(
                url.host_str(),
                Some("nodejs.org" | "registry.npmjs.org" | "cdn.playwright.dev")
            ));
            assert_eq!(artifact.sha256.len(), 64);
        }
        assert!(target().is_ok());
    }
}
