//! GitHub repository storage for cloud sync. Files live on one branch and are
//! written through the Contents API, whose `sha` check gives the conditional
//! write the sync engine needs. Reads use the JSON representation for small
//! files and the raw representation above its inline limit.
use super::{
    store::{normalize_prefix, probe_store, valid_file, Download, RemoteStore},
    transport::read_response,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::{Client, Method, Response, StatusCode, Url};
use serde::Deserialize;

const API_ROOT: &str = "https://api.github.com";
const COMMIT_MESSAGE: &str = "BalanceHub 云端同步";
/// GitHub rejects file writes above 100 MB; keep a margin for the base64
/// request body and report the limit instead of a misleading write conflict.
const MAX_FILE_BYTES: usize = 95_000_000;
pub(super) struct GithubStore {
    client: Client,
    owner: String,
    repo: String,
    branch: String,
    prefix: String,
    token: String,
}

fn transport_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "GitHub 请求超时，请检查网络后重试".to_owned()
    } else if error.is_connect() {
        "无法连接 GitHub，请检查代理和证书".to_owned()
    } else {
        "GitHub 请求失败，请检查网络后重试".to_owned()
    }
}

fn status_error(status: StatusCode) -> String {
    match status {
        StatusCode::UNAUTHORIZED => "GitHub 令牌无效或已过期，请重新填写".to_owned(),
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS => {
            "GitHub 拒绝访问：请确认令牌拥有该仓库的 Contents 读写权限，且请求未超过频率限制"
                .to_owned()
        }
        StatusCode::NOT_FOUND => "GitHub 仓库或分支不存在，或令牌无权访问该仓库".to_owned(),
        StatusCode::UNPROCESSABLE_ENTITY => {
            "GitHub 未接受这次写入，请检查仓库是否为私有且默认分支可写".to_owned()
        }
        _ => format!("GitHub 返回 HTTP {}，请稍后重试", status.as_u16()),
    }
}

fn valid_owner(value: &str) -> bool {
    let value = value.trim();
    (1..=100).contains(&value.len())
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_repo(value: &str) -> bool {
    let value = value.trim();
    (1..=100).contains(&value.len())
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_branch(value: &str) -> bool {
    let value = value.trim();
    (1..=255).contains(&value.len())
        && !value.starts_with(['/', '.'])
        && !value.ends_with(['/', '.'])
        && !value.contains("..")
        && !value.contains("//")
        && !value.contains("@{")
        && value.bytes().all(|byte| {
            byte.is_ascii_graphic()
                && !matches!(byte, b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\')
        })
}

pub(super) fn validate(
    owner: &str,
    repo: &str,
    branch: &str,
    prefix: &str,
    token: &str,
) -> Result<(), String> {
    if !valid_owner(owner) {
        return Err("GitHub 仓库所有者无效，请填写用户名或组织名".to_owned());
    }
    if !valid_repo(repo) {
        return Err("GitHub 仓库名称无效，只能包含字母、数字、-、_ 和 .".to_owned());
    }
    if !valid_branch(branch) {
        return Err("GitHub 分支名称无效".to_owned());
    }
    normalize_prefix(prefix)?;
    if token.trim().is_empty() {
        return Err("请填写 GitHub 个人访问令牌".to_owned());
    }
    Ok(())
}

fn unquote(value: &str) -> &str {
    value.trim_matches('"')
}

#[derive(Deserialize)]
struct ContentFile {
    sha: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    encoding: String,
    #[serde(default)]
    content: String,
}

impl GithubStore {
    pub fn new(
        client: Client,
        owner: &str,
        repo: &str,
        branch: &str,
        prefix: &str,
        token: &str,
    ) -> Result<Self, String> {
        validate(owner, repo, branch, prefix, token)?;
        Ok(Self {
            client,
            owner: owner.trim().to_owned(),
            repo: repo.trim().to_owned(),
            branch: branch.trim().to_owned(),
            prefix: normalize_prefix(prefix)?,
            token: token.to_owned(),
        })
    }

    fn contents_url(&self, file: &str, with_ref: bool) -> Result<Url, String> {
        if !valid_file(file) {
            return Err("同步对象路径无效".to_owned());
        }
        let mut url = Url::parse(API_ROOT).map_err(|_| "GitHub API 地址无效")?;
        {
            let mut path = url.path_segments_mut().map_err(|_| "GitHub API 地址无效")?;
            path.push("repos")
                .push(&self.owner)
                .push(&self.repo)
                .push("contents");
            if !self.prefix.is_empty() {
                for part in self.prefix.split('/') {
                    path.push(part);
                }
            }
            for part in file.split('/') {
                path.push(part);
            }
        }
        if with_ref {
            url.query_pairs_mut().append_pair("ref", &self.branch);
        }
        Ok(url)
    }

    fn request(&self, method: Method, url: Url) -> reqwest::RequestBuilder {
        self.client
            .request(method, url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "BalanceHub")
    }

    async fn file_sha(&self, file: &str) -> Result<Option<String>, String> {
        let response = self
            .request(Method::GET, self.contents_url(file, true)?)
            .send()
            .await
            .map_err(transport_error)?;
        match response.status() {
            StatusCode::NOT_FOUND => Ok(None),
            status if status.is_success() => {
                let file: ContentFile = response.json().await.map_err(transport_error)?;
                Ok(Some(file.sha))
            }
            status => Err(status_error(status)),
        }
    }

    async fn write(
        &self,
        method: Method,
        file: &str,
        body: serde_json::Value,
    ) -> Result<Response, String> {
        self.request(method, self.contents_url(file, false)?)
            .json(&body)
            .send()
            .await
            .map_err(transport_error)
    }
}

#[async_trait::async_trait]
impl RemoteStore for GithubStore {
    fn label(&self) -> &'static str {
        "GitHub"
    }

    async fn test(&self) -> Result<(), String> {
        probe_store(self).await
    }

    async fn get(
        &self,
        file: &str,
        if_none_match: Option<&str>,
        limit: usize,
    ) -> Result<Download, String> {
        let mut request = self.request(Method::GET, self.contents_url(file, true)?);
        if let Some(etag) = if_none_match {
            request = request.header("If-None-Match", etag);
        }
        let response = request.send().await.map_err(transport_error)?;
        match response.status() {
            StatusCode::NOT_FOUND => return Ok(Download::Missing),
            StatusCode::NOT_MODIFIED => return Ok(Download::Unchanged),
            status if !status.is_success() => return Err(status_error(status)),
            _ => {}
        }
        let metadata: ContentFile = response.json().await.map_err(transport_error)?;
        if metadata.size > limit as u64 {
            return Err("同步文件超过支持的大小，未应用远端数据".to_owned());
        }
        let bytes = if metadata.encoding == "base64" {
            let encoded: String = metadata
                .content
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            STANDARD
                .decode(encoded)
                .map_err(|_| "GitHub 返回的文件内容无法解码")?
        } else {
            let response = self
                .request(Method::GET, self.contents_url(file, true)?)
                .header("Accept", "application/vnd.github.raw+json")
                .send()
                .await
                .map_err(transport_error)?;
            let status = response.status();
            if !status.is_success() {
                return Err(status_error(status));
            }
            read_response(response, limit).await?
        };
        if bytes.len() > limit {
            return Err("同步文件超过支持的大小，未应用远端数据".to_owned());
        }
        Ok(Download::Found {
            bytes,
            etag: format!("\"{}\"", metadata.sha),
        })
    }

    async fn put(
        &self,
        file: &str,
        bytes: Vec<u8>,
        expected: Option<&str>,
    ) -> Result<bool, String> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err("GitHub 单个同步文件不能超过 95 MB".to_owned());
        }
        let mut body = serde_json::json!({
            "message": COMMIT_MESSAGE,
            "content": STANDARD.encode(bytes),
            "branch": self.branch,
        });
        if let Some(etag) = expected {
            body["sha"] = serde_json::Value::String(unquote(etag).to_owned());
        }
        let response = self.write(Method::PUT, file, body).await?;
        match response.status() {
            status if status.is_success() => Ok(true),
            // An update without the current file sha and one with a stale sha
            // both mean another device got there first.
            StatusCode::CONFLICT => Ok(false),
            StatusCode::UNPROCESSABLE_ENTITY => {
                let detail = response.text().await.unwrap_or_default();
                let detail = detail.trim();
                if detail.to_ascii_lowercase().contains("sha") {
                    Ok(false)
                } else if detail.is_empty() {
                    Err(status_error(StatusCode::UNPROCESSABLE_ENTITY))
                } else {
                    Err(format!("GitHub 未接受这次写入：{detail}"))
                }
            }
            status => Err(status_error(status)),
        }
    }

    async fn remove(&self, file: &str) -> Result<(), String> {
        let Some(sha) = self.file_sha(file).await? else {
            return Ok(());
        };
        let body = serde_json::json!({
            "message": COMMIT_MESSAGE,
            "sha": sha,
            "branch": self.branch,
        });
        let response = self.write(Method::DELETE, file, body).await?;
        match response.status() {
            status if status.is_success() || status == StatusCode::NOT_FOUND => Ok(()),
            StatusCode::CONFLICT | StatusCode::UNPROCESSABLE_ENTITY => {
                Err("云端文件已变化，请重新同步".to_owned())
            }
            status => Err(status_error(status)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> GithubStore {
        GithubStore::new(
            Client::new(),
            "notochen",
            "balancehub-sync",
            "main",
            "BalanceHub",
            "fixture-token",
        )
        .unwrap()
    }

    #[test]
    fn contents_urls_keep_the_prefix_and_branch_escape_safe() {
        let store = store();
        assert_eq!(
            store.contents_url("head.json", true).unwrap().as_str(),
            "https://api.github.com/repos/notochen/balancehub-sync/contents/BalanceHub/head.json?ref=main"
        );
        assert_eq!(
            store.contents_url("objects/abc", false).unwrap().as_str(),
            "https://api.github.com/repos/notochen/balancehub-sync/contents/BalanceHub/objects/abc"
        );
        let nested = GithubStore::new(
            Client::new(),
            "noto-chen",
            "data",
            "feature/sync",
            "sync/v1",
            "token",
        )
        .unwrap();
        assert_eq!(
            nested.contents_url("head.json", true).unwrap().as_str(),
            "https://api.github.com/repos/noto-chen/data/contents/sync/v1/head.json?ref=feature%2Fsync"
        );
    }

    #[test]
    fn invalid_owner_repo_branch_prefix_and_token_are_rejected() {
        let client = Client::new;
        assert!(GithubStore::new(client(), "", "repo", "main", "", "token").is_err());
        assert!(GithubStore::new(client(), "-owner", "repo", "main", "", "token").is_err());
        assert!(GithubStore::new(client(), "owner", "bad repo", "main", "", "token").is_err());
        assert!(GithubStore::new(client(), "owner", "repo", "..", "", "token").is_err());
        assert!(GithubStore::new(client(), "owner", "repo", "main", "a b", "token").is_err());
        assert!(GithubStore::new(client(), "owner", "repo", "main", "", " ").is_err());
        assert!(store().contents_url("../escape", true).is_err());
    }
}
