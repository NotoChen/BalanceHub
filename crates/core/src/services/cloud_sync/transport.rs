use super::format::random_id;
use reqwest::{Client, Method, Response, StatusCode, Url};

#[derive(Clone)]
pub(super) struct DavClient {
    client: Client,
    root: Url,
    directories: Vec<Url>,
    username: String,
    password: String,
}

pub(super) enum Download {
    Missing,
    Unchanged,
    Found { bytes: Vec<u8>, etag: String },
}

fn transport_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "WebDAV 请求超时，请检查网络后重试".to_owned()
    } else if error.is_connect() {
        "无法连接 WebDAV，请检查地址、代理和证书".to_owned()
    } else {
        "WebDAV 请求失败，请检查网络后重试".to_owned()
    }
}

fn status_error(status: StatusCode) -> String {
    match status {
        StatusCode::UNAUTHORIZED => "WebDAV 认证失败，请检查用户名和应用密码".to_owned(),
        StatusCode::FORBIDDEN => "WebDAV 目录无访问权限，请检查目录读写授权".to_owned(),
        StatusCode::NOT_FOUND => "WebDAV 地址不存在，请检查服务地址及上级目录".to_owned(),
        StatusCode::INSUFFICIENT_STORAGE => "WebDAV 存储空间不足".to_owned(),
        _ => format!("WebDAV 返回 HTTP {}，请稍后重试", status.as_u16()),
    }
}

pub(super) fn normalize_url(raw: &str, remote_root: &str) -> Result<Url, String> {
    let mut url = Url::parse(raw.trim()).map_err(|_| "请输入有效的 WebDAV 地址")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("WebDAV 地址须为 HTTP(S) 目录地址，用户名和密码请填写在独立输入框".to_owned());
    }
    let root = remote_root.trim().trim_matches('/');
    if root.is_empty()
        || root.split('/').any(|part| {
            part.is_empty()
                || matches!(part, "." | "..")
                || part.contains('\\')
                || part.chars().any(char::is_control)
        })
    {
        return Err("同步目录无效，请填写独立的相对目录，例如 BalanceHub".to_owned());
    }
    let mut path = url
        .path_segments_mut()
        .map_err(|_| "WebDAV 地址无法追加目录")?;
    path.pop_if_empty();
    for segment in root.split('/') {
        path.push(segment);
    }
    path.push("");
    drop(path);
    Ok(url)
}

impl DavClient {
    pub fn new(
        client: Client,
        server_url: &str,
        remote_root: &str,
        username: &str,
        password: &str,
    ) -> Result<Self, String> {
        let root = normalize_url(server_url, remote_root)?;
        let mut directory = Url::parse(server_url.trim()).map_err(|_| "WebDAV 地址无效")?;
        let mut directories = Vec::new();
        for segment in remote_root
            .trim()
            .trim_matches('/')
            .split('/')
            .chain(std::iter::once("objects"))
        {
            directory
                .path_segments_mut()
                .map_err(|_| "同步目录无效")?
                .pop_if_empty()
                .push(segment)
                .push("");
            directories.push(directory.clone());
        }
        Ok(Self {
            client,
            root,
            directories,
            username: username.to_owned(),
            password: password.to_owned(),
        })
    }

    fn url(&self, file: &str) -> Result<Url, String> {
        if file
            .split('/')
            .any(|part| matches!(part, "." | "..") || part.contains('\\'))
        {
            return Err("同步对象路径无效".to_owned());
        }
        self.root
            .join(file)
            .map_err(|_| "同步对象地址无效".to_owned())
    }

    fn request(&self, method: Method, url: Url) -> reqwest::RequestBuilder {
        let request = self.client.request(method, url);
        if self.username.is_empty() {
            request
        } else {
            request.basic_auth(&self.username, Some(&self.password))
        }
    }

    pub async fn ensure_directories(&self) -> Result<(), String> {
        for url in &self.directories {
            let response = self
                .request(
                    Method::from_bytes(b"MKCOL").map_err(|_| "WebDAV 方法无效")?,
                    url.clone(),
                )
                .send()
                .await
                .map_err(transport_error)?;
            if response.status().is_success() {
                continue;
            }
            if !matches!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED | StatusCode::CONFLICT
            ) {
                return Err(status_error(response.status()));
            }
            let probe = self
                .request(
                    Method::from_bytes(b"PROPFIND").map_err(|_| "WebDAV 方法无效")?,
                    url.clone(),
                )
                .header("Depth", "0")
                .send()
                .await
                .map_err(transport_error)?;
            if !probe.status().is_success() {
                return Err(status_error(probe.status()));
            }
        }
        Ok(())
    }

    pub async fn get(
        &self,
        file: &str,
        if_none_match: Option<&str>,
        limit: usize,
    ) -> Result<Download, String> {
        let mut request = self.request(Method::GET, self.url(file)?);
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
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        if etag.is_empty() || etag.starts_with("W/") {
            return Err("WebDAV 未提供强 ETag，无法安全协调多设备同步".to_owned());
        }
        Ok(Download::Found {
            bytes: read_response(response, limit).await?,
            etag,
        })
    }

    /// false means the remote changed; never retry this write without re-reading.
    pub async fn put(
        &self,
        file: &str,
        bytes: Vec<u8>,
        expected: Option<&str>,
    ) -> Result<bool, String> {
        let mut request = self
            .request(Method::PUT, self.url(file)?)
            .header("Content-Type", "application/octet-stream")
            .body(bytes);
        request = match expected {
            Some(etag) => request.header("If-Match", etag),
            None => request.header("If-None-Match", "*"),
        };
        let response = request.send().await.map_err(transport_error)?;
        if response.status() == StatusCode::PRECONDITION_FAILED {
            return Ok(false);
        }
        if !response.status().is_success() {
            return Err(status_error(response.status()));
        }
        Ok(true)
    }

    async fn delete_probe(&self, name: &str) -> Result<(), String> {
        let response = self
            .request(Method::DELETE, self.url(name)?)
            .send()
            .await
            .map_err(transport_error)?;
        if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(status_error(response.status()))
        }
    }

    /// Check the actual read/write and conditional-write contract using only
    /// a random temporary object; never change the user's current snapshot.
    pub async fn test(&self) -> Result<(), String> {
        self.ensure_directories().await?;
        let name = format!(".balancehub-probe-{}", random_id()?);
        let result = self.test_probe(&name).await;
        let cleanup = self.delete_probe(&name).await;
        result?;
        cleanup.map_err(|_| "连接验证通过，但临时探测文件清理失败，请检查删除权限".to_owned())
    }

    async fn test_probe(&self, name: &str) -> Result<(), String> {
        let content = b"BalanceHub WebDAV capability probe".to_vec();
        if !self.put(name, content.clone(), None).await? {
            return Err("WebDAV 不支持创建同步文件".to_owned());
        }
        let mut etag = match self.get(name, None, 1024).await? {
            Download::Found { bytes, etag } if bytes == content => etag,
            _ => return Err("WebDAV 写入后读取内容不一致".to_owned()),
        };
        if self
            .put(name, content.clone(), Some("\"balancehub-invalid-etag\""))
            .await?
            || self.put(name, content.clone(), None).await?
        {
            return Err(
                "该 WebDAV 服务忽略条件写入，无法防止多设备互相覆盖，请使用支持 If-Match 的服务"
                    .to_owned(),
            );
        }
        // A timestamp/size ETag can claim to be strong while missing rapid
        // same-size updates (including encrypted manifests). Check this before
        // trusting either conditional reads or writes for user data.
        for marker in *b"123" {
            let mut replacement = content.clone();
            replacement[0] = marker;
            if !self.put(name, replacement.clone(), Some(&etag)).await? {
                return Err("WebDAV 条件写入验证失败".to_owned());
            }
            let next_etag = match self.get(name, None, 1024).await? {
                Download::Found { bytes, etag: next } if bytes == replacement && next != etag => {
                    next
                }
                Download::Found { bytes, .. } if bytes == replacement => return Err(
                    "WebDAV 的文件版本标识未随内容变化，无法安全同步；请使用提供可靠强 ETag 的服务"
                        .to_owned(),
                ),
                _ => return Err("WebDAV 条件写入后内容不一致".to_owned()),
            };
            if self.put(name, content.clone(), Some(&etag)).await? {
                return Err("WebDAV 接受了过期的文件版本，无法安全协调多设备同步".to_owned());
            }
            match self.get(name, Some(&etag), 1024).await? {
                Download::Found { bytes, .. } if bytes == replacement => {}
                _ => return Err("WebDAV 条件读取未返回最新内容，无法安全同步".to_owned()),
            }
            etag = next_etag;
        }
        Ok(())
    }
}

async fn read_response(mut response: Response, limit: usize) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err("同步文件超过支持的大小，未应用远端数据".to_owned());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err("同步文件超过支持的大小，未应用远端数据".to_owned());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
