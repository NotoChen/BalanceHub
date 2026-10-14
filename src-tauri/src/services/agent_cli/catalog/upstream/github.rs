use crate::{
    models::AgentSkillOrigin,
    services::agent_cli::catalog::{definition::PackageFile, package},
};
use futures_util::{stream, StreamExt, TryStreamExt};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(super) struct Head {
    pub revision: String,
    tree: String,
}
type HeadSlot = (Instant, Arc<tokio::sync::OnceCell<Result<Head, String>>>);
#[derive(Default)]
pub(in crate::services::agent_cli::catalog) struct UpstreamCache {
    heads: Mutex<BTreeMap<String, HeadSlot>>,
}

pub(in crate::services::agent_cli::catalog) fn normalize(
    mut origin: AgentSkillOrigin,
) -> Result<AgentSkillOrigin, String> {
    let url = reqwest::Url::parse(origin.repository.trim())
        .map_err(|_| "请输入完整的 GitHub 仓库地址")?;
    let path = url.path().trim_matches('/').trim_end_matches(".git");
    let parts = path.split('/').collect::<Vec<_>>();
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || !part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
    {
        return Err(
            "来源目前支持 https://github.com/作者/仓库，请单独填写分支和 Skill 目录".to_owned(),
        );
    }
    origin.repository = format!("https://github.com/{path}");
    origin.reference = origin.reference.trim().to_owned();
    if origin.reference.len() > 256 || origin.reference.chars().any(char::is_control) {
        return Err("分支或版本名称无效".to_owned());
    }
    origin.directory = origin.directory.trim().trim_matches('/').to_owned();
    if !origin.directory.is_empty() {
        package::validate_relative(&origin.directory)?;
    }
    if origin
        .revision
        .as_deref()
        .is_some_and(|revision| !git_hash(revision))
    {
        return Err("来源版本标识无效".to_owned());
    }
    Ok(origin)
}

fn endpoint(origin: &AgentSkillOrigin, parts: &[&str]) -> Result<reqwest::Url, String> {
    let normalized = normalize(origin.clone())?;
    let repository = normalized
        .repository
        .trim_start_matches("https://github.com/");
    let mut url = reqwest::Url::parse(&format!("https://api.github.com/repos/{repository}/"))
        .map_err(|_| "仓库地址无效")?;
    url.path_segments_mut()
        .map_err(|_| "仓库地址无效")?
        .pop_if_empty()
        .extend(parts.iter().copied());
    Ok(url)
}

async fn bytes(
    client: &reqwest::Client,
    url: reqwest::Url,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let mut response = client
        .get(url)
        .header("User-Agent", "BalanceHub")
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                "连接来源仓库超时"
            } else {
                "无法连接来源仓库，请检查代理或网络"
            }
        })?;
    if !response.status().is_success() {
        return Err(match response.status().as_u16() {
            403 | 429 => "GitHub 请求受限，请稍后再试".to_owned(),
            404 => "仓库、分支或 Skill 目录不存在，或仓库不是公开仓库".to_owned(),
            status => format!("来源仓库返回 HTTP {status}"),
        });
    }
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err("上游文件超过读取预算".to_owned());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "读取上游内容失败")? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err("上游文件超过读取预算".to_owned());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
async fn json(client: &reqwest::Client, url: reqwest::Url) -> Result<serde_json::Value, String> {
    serde_json::from_slice(&bytes(client, url, 8 * 1024 * 1024).await?)
        .map_err(|_| "来源仓库返回了无效数据".to_owned())
}
impl UpstreamCache {
    pub(super) async fn head(
        &self,
        client: &reqwest::Client,
        origin: &AgentSkillOrigin,
    ) -> Result<Head, String> {
        let reference = if origin.reference.is_empty() {
            "HEAD"
        } else {
            &origin.reference
        };
        let key = format!("{}:{reference}:{}", origin.repository, origin.directory);
        let slot = {
            let mut heads = self.heads.lock().map_err(|_| "上游查询缓存不可用")?;
            heads.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(30));
            if heads.len() >= 128 {
                heads.clear();
            }
            Arc::clone(
                &heads
                    .entry(key)
                    .or_insert_with(|| (Instant::now(), Arc::new(tokio::sync::OnceCell::new())))
                    .1,
            )
        };
        slot.get_or_init(|| async {
            let mut url = endpoint(origin, &["commits"])?;
            url.query_pairs_mut()
                .append_pair("sha", reference)
                .append_pair("per_page", "1");
            if !origin.directory.is_empty() {
                url.query_pairs_mut().append_pair("path", &origin.directory);
            }
            let values = json(client, url).await?;
            let value = values
                .as_array()
                .and_then(|values| values.first())
                .ok_or("选定目录没有可读取的提交")?;
            let revision = value
                .get("sha")
                .and_then(|value| value.as_str())
                .ok_or("上游版本无效")?
                .to_owned();
            let tree = value
                .pointer("/commit/tree/sha")
                .and_then(|value| value.as_str())
                .ok_or("上游文件树无效")?
                .to_owned();
            if !git_hash(&revision) || !git_hash(&tree) {
                return Err("上游版本标识无效".to_owned());
            }
            Ok(Head { revision, tree })
        })
        .await
        .clone()
    }
}
fn git_hash(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Deserialize)]
struct Tree {
    tree: Vec<TreeEntry>,
    #[serde(default)]
    truncated: bool,
}
#[derive(Deserialize)]
struct TreeEntry {
    path: String,
    mode: String,
    r#type: String,
    sha: String,
    size: Option<usize>,
}
async fn tree(
    client: &reqwest::Client,
    origin: &AgentSkillOrigin,
    sha: &str,
    recursive: bool,
) -> Result<Tree, String> {
    let mut url = endpoint(origin, &["git", "trees", sha])?;
    if recursive {
        url.query_pairs_mut().append_pair("recursive", "1");
    }
    let tree: Tree =
        serde_json::from_value(json(client, url).await?).map_err(|_| "上游目录格式无效")?;
    if tree.truncated {
        return Err("上游目录清单未完整返回，请选择更具体的 Skill 子目录".to_owned());
    }
    Ok(tree)
}
pub(super) async fn download(
    client: &reqwest::Client,
    origin: &AgentSkillOrigin,
    head: &Head,
) -> Result<BTreeMap<String, PackageFile>, String> {
    let mut sha = head.tree.clone();
    for directory in origin.directory.split('/').filter(|part| !part.is_empty()) {
        sha = tree(client, origin, &sha, false)
            .await?
            .tree
            .into_iter()
            .find(|entry| entry.path == directory && entry.r#type == "tree")
            .ok_or("Skill 目录不存在")?
            .sha;
        if !git_hash(&sha) {
            return Err("Skill 目录标识无效".to_owned());
        }
    }
    let entries = tree(client, origin, &sha, true).await?.tree;
    let mut selected = Vec::new();
    let mut size = 0usize;
    for entry in entries {
        if entry
            .path
            .split('/')
            .any(|part| matches!(part, ".git" | "node_modules" | "__pycache__" | ".venv"))
        {
            continue;
        }
        if entry.r#type == "tree" {
            continue;
        }
        package::validate_relative(&entry.path)?;
        if entry.r#type != "blob" || !matches!(entry.mode.as_str(), "100644" | "100755") {
            return Err("Skill 包含链接或子模块，请使用包含独立文件的 Skill 目录".to_owned());
        }
        let bytes = entry.size.ok_or("上游文件缺少长度")?;
        size = size.saturating_add(bytes);
        if bytes > package::MAX_FILE_BYTES
            || size > package::MAX_PACKAGE_BYTES
            || selected.len() >= package::MAX_PACKAGE_FILES
        {
            return Err("上游 Skill 超过 64 MiB、8192 个文件或单文件 16 MiB 预算".to_owned());
        }
        selected.push(entry);
    }
    let files = stream::iter(selected.into_iter().map(|entry| async move {
        let repository = origin.repository.trim_start_matches("https://github.com/");
        let mut url =
            reqwest::Url::parse(&format!("https://raw.githubusercontent.com/{repository}/"))
                .map_err(|_| "仓库地址无效")?;
        url.path_segments_mut()
            .map_err(|_| "仓库地址无效")?
            .pop_if_empty()
            .push(&head.revision)
            .extend(origin.directory.split('/').filter(|part| !part.is_empty()))
            .extend(entry.path.split('/'));
        let content = bytes(client, url, package::MAX_FILE_BYTES).await?;
        if Some(content.len()) != entry.size {
            return Err("上游文件长度与目录记录不一致".to_owned());
        }
        use sha2::Digest;
        let header = format!("blob {}\0", content.len());
        let fingerprint = match entry.sha.len() {
            40 => {
                let mut digest = sha1::Sha1::new();
                digest.update(header.as_bytes());
                digest.update(&content);
                format!("{:x}", digest.finalize())
            }
            64 => {
                let mut digest = sha2::Sha256::new();
                digest.update(header.as_bytes());
                digest.update(&content);
                format!("{:x}", digest.finalize())
            }
            _ => return Err("上游文件标识无效".to_owned()),
        };
        if fingerprint != entry.sha {
            return Err("上游文件未通过 Git 内容校验".to_owned());
        }
        Ok((
            entry.path,
            PackageFile {
                bytes: content.into(),
                executable: entry.mode == "100755",
            },
        ))
    }))
    .buffer_unordered(4)
    .try_collect::<BTreeMap<_, _>>()
    .await?;
    let manifest = files.get("SKILL.md").ok_or("选定目录中没有 SKILL.md")?;
    crate::services::agent_cli::catalog::definition::validate_skill(&manifest.bytes)?;
    Ok(files)
}
