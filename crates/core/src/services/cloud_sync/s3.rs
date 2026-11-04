//! S3-compatible object storage (AWS S3, Cloudflare R2, MinIO, Backblaze B2,
//! ...). Requests use SigV4 with path-style addressing on custom endpoints and
//! virtual-hosted addressing on AWS. Conditional writes are mandatory: the
//! capability probe rejects providers that silently overwrite.
use super::store::{probe_store, Download, RemoteStore};
use super::transport::read_response;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use reqwest::{Client, Method, StatusCode, Url};
use sha2::{Digest, Sha256};

const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

pub(super) struct S3Store {
    client: Client,
    endpoint: Url,
    bucket: String,
    prefix: String,
    region: String,
    access_key: String,
    secret_key: String,
    virtual_host: bool,
}

fn transport_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "S3 请求超时，请检查网络后重试".to_owned()
    } else if error.is_connect() {
        "无法连接 S3 存储，请检查服务地址、代理和证书".to_owned()
    } else {
        "S3 请求失败，请检查网络后重试".to_owned()
    }
}

fn status_error(status: StatusCode, bucket: &str) -> String {
    match status {
        StatusCode::UNAUTHORIZED => "S3 认证失败，请检查 Access Key 与 Secret Key".to_owned(),
        StatusCode::FORBIDDEN => format!(
            "S3 拒绝访问：请确认凭据拥有 s3:GetObject、s3:PutObject、s3:DeleteObject 与 s3:ListBucket（{bucket}）权限"
        ),
        StatusCode::NOT_FOUND => format!("S3 存储桶 {bucket} 不存在，请检查桶名与区域"),
        StatusCode::MOVED_PERMANENTLY | StatusCode::TEMPORARY_REDIRECT => {
            "S3 存储桶不在所选区域，请检查区域设置".to_owned()
        }
        StatusCode::BAD_REQUEST | StatusCode::NOT_IMPLEMENTED => format!(
            "S3 服务未接受条件写入（HTTP {}），请确认支持 If-Match 与 If-None-Match",
            status.as_u16()
        ),
        _ => format!("S3 返回 HTTP {}，请稍后重试", status.as_u16()),
    }
}

pub(super) fn valid_bucket(bucket: &str) -> bool {
    (3..=63).contains(&bucket.len())
        && bucket.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.'
        })
        && !bucket.starts_with(['-', '.'])
        && !bucket.ends_with(['-', '.'])
        && !bucket.contains("..")
}

/// Prefix segments stay inside the canonical-URI unreserved set so signing
/// never has to re-encode a user path.
pub(super) fn normalize_prefix(raw: &str) -> Result<String, String> {
    let prefix = raw.trim().trim_matches('/');
    if prefix.is_empty() {
        return Ok(prefix.to_owned());
    }
    if prefix.split('/').any(|part| {
        part.is_empty()
            || matches!(part, "." | "..")
            || !part.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
            })
    }) {
        return Err("同步前缀只能包含字母、数字、-、_、. 与 /".to_owned());
    }
    Ok(prefix.to_owned())
}

pub(super) fn normalize_endpoint(raw: &str, region: &str) -> Result<Url, String> {
    let raw = raw.trim();
    let raw = if raw.is_empty() {
        if region.is_empty() {
            return Err("请填写 S3 服务地址或区域".to_owned());
        }
        if region == "auto" {
            return Err("使用 Cloudflare R2 等自定义服务时，请填写 S3 服务地址".to_owned());
        }
        if region == "us-east-1" {
            "https://s3.us-east-1.amazonaws.com".to_owned()
        } else {
            format!("https://s3.{region}.amazonaws.com")
        }
    } else {
        raw.to_owned()
    };
    let url = Url::parse(&raw).map_err(|_| "请输入有效的 S3 服务地址")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("S3 服务地址须为 HTTP(S) 地址，凭据请填写在独立输入框".to_owned());
    }
    Ok(url)
}

fn valid_file(file: &str) -> bool {
    !file.is_empty()
        && !file.starts_with('/')
        && file.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
                })
        })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hmac(key: &[u8], value: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(value);
    mac.finalize().into_bytes().to_vec()
}

fn host_header(url: &Url) -> Result<String, String> {
    let host = url.host_str().ok_or("S3 服务地址缺少主机名")?;
    let host = if host.contains(':') {
        // url exposes IPv6 literals without brackets; a Host header needs them.
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    Ok(match url.port() {
        Some(port) if !is_default_port(url.scheme(), port) => format!("{host}:{port}"),
        _ => host,
    })
}

fn is_default_port(scheme: &str, port: u16) -> bool {
    (scheme == "https" && port == 443) || (scheme == "http" && port == 80)
}

pub(super) fn validate(
    endpoint: &str,
    bucket: &str,
    prefix: &str,
    region: &str,
    access_key: &str,
) -> Result<(), String> {
    if !valid_bucket(bucket.trim()) {
        return Err(
            "S3 存储桶名称无效：需 3–63 位小写字母、数字、- 或 .，且首尾不能是符号".to_owned(),
        );
    }
    if access_key.trim().is_empty() {
        return Err("请填写 S3 Access Key".to_owned());
    }
    normalize_endpoint(endpoint, region.trim())?;
    normalize_prefix(prefix)?;
    Ok(())
}

impl S3Store {
    pub fn new(
        client: Client,
        endpoint: &str,
        bucket: &str,
        prefix: &str,
        region: &str,
        access_key: &str,
        secret_key: &str,
    ) -> Result<Self, String> {
        validate(endpoint, bucket, prefix, region, access_key)?;
        let region = region.trim();
        let endpoint = normalize_endpoint(endpoint, region)?;
        let virtual_host = endpoint
            .host_str()
            .is_some_and(|host| host == "s3.amazonaws.com" || host.ends_with(".amazonaws.com"));
        Ok(Self {
            client,
            endpoint,
            bucket: bucket.trim().to_owned(),
            prefix: normalize_prefix(prefix)?,
            region: if region.is_empty() {
                "us-east-1".to_owned()
            } else {
                region.to_owned()
            },
            access_key: access_key.trim().to_owned(),
            secret_key: secret_key.to_owned(),
            virtual_host,
        })
    }

    fn object_url(&self, file: &str) -> Result<Url, String> {
        if !valid_file(file) {
            return Err("同步对象路径无效".to_owned());
        }
        let mut url = self.endpoint.clone();
        let mut segments = Vec::new();
        if self.virtual_host {
            let host = url.host_str().ok_or("S3 服务地址缺少主机名")?.to_owned();
            url.set_host(Some(&format!("{}.{}", self.bucket, host)))
                .map_err(|_| "S3 存储桶名称无法用于服务地址".to_owned())?;
        } else {
            segments.push(self.bucket.as_str());
        }
        if !self.prefix.is_empty() {
            segments.extend(self.prefix.split('/'));
        }
        segments.extend(file.split('/'));
        let mut path = url.path_segments_mut().map_err(|_| "S3 服务地址无效")?;
        path.pop_if_empty();
        for segment in segments {
            path.push(segment);
        }
        drop(path);
        Ok(url)
    }

    fn sign(
        &self,
        method: &str,
        url: &Url,
        payload_hash: &str,
        at: DateTime<Utc>,
    ) -> Result<Vec<(String, String)>, String> {
        let date = at.format("%Y%m%d").to_string();
        let amz_date = at.format("%Y%m%dT%H%M%SZ").to_string();
        let canonical_uri = url.path();
        let host = host_header(url)?;
        let canonical_headers =
            format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!(
            "{method}\n{canonical_uri}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
        );
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let key = hmac(
            format!("AWS4{}", self.secret_key).as_bytes(),
            date.as_bytes(),
        );
        let key = hmac(&key, self.region.as_bytes());
        let key = hmac(&key, b"s3");
        let key = hmac(&key, b"aws4_request");
        let signature = hex(&hmac(&key, string_to_sign.as_bytes()));
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key
        );
        Ok(vec![
            ("x-amz-date".to_owned(), amz_date),
            ("x-amz-content-sha256".to_owned(), payload_hash.to_owned()),
            ("authorization".to_owned(), authorization),
        ])
    }

    fn request(
        &self,
        method: Method,
        url: Url,
        payload_hash: &str,
    ) -> Result<reqwest::RequestBuilder, String> {
        let headers = self.sign(method.as_str(), &url, payload_hash, Utc::now())?;
        let mut request = self.client.request(method, url);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        Ok(request)
    }
}

#[async_trait::async_trait]
impl RemoteStore for S3Store {
    fn label(&self) -> &'static str {
        "S3"
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
        let mut request = self.request(Method::GET, self.object_url(file)?, EMPTY_SHA256)?;
        if let Some(etag) = if_none_match {
            request = request.header("If-None-Match", etag);
        }
        let response = request.send().await.map_err(transport_error)?;
        match response.status() {
            StatusCode::NOT_FOUND => return Ok(Download::Missing),
            StatusCode::NOT_MODIFIED => return Ok(Download::Unchanged),
            status if !status.is_success() => {
                return Err(status_error(status, &self.bucket));
            }
            _ => {}
        }
        let etag = response
            .headers()
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .filter(|value| !value.is_empty())
            .ok_or("S3 未返回 ETag，无法安全协调多设备同步")?;
        Ok(Download::Found {
            bytes: read_response(response, limit).await?,
            etag,
        })
    }

    async fn put(
        &self,
        file: &str,
        bytes: Vec<u8>,
        expected: Option<&str>,
    ) -> Result<bool, String> {
        let payload_hash = sha256_hex(&bytes);
        let mut request = self.request(Method::PUT, self.object_url(file)?, &payload_hash)?;
        request = match expected {
            Some(etag) => request.header("If-Match", etag),
            None => request.header("If-None-Match", "*"),
        };
        let response = request.body(bytes).send().await.map_err(transport_error)?;
        if response.status() == StatusCode::PRECONDITION_FAILED {
            return Ok(false);
        }
        if !response.status().is_success() {
            return Err(status_error(response.status(), &self.bucket));
        }
        Ok(true)
    }

    async fn remove(&self, file: &str) -> Result<(), String> {
        let response = self
            .request(Method::DELETE, self.object_url(file)?, EMPTY_SHA256)?
            .send()
            .await
            .map_err(transport_error)?;
        if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(status_error(response.status(), &self.bucket))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(endpoint: &str, prefix: &str) -> S3Store {
        S3Store::new(
            Client::new(),
            endpoint,
            "balancehub-data",
            prefix,
            "us-east-1",
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
        )
        .unwrap()
    }

    #[test]
    fn aws_endpoints_use_virtual_hosts_and_shared_uris() {
        let aws = store("", "BalanceHub");
        assert_eq!(
            aws.object_url("head.json").unwrap().as_str(),
            "https://balancehub-data.s3.us-east-1.amazonaws.com/BalanceHub/head.json"
        );
        assert_eq!(
            aws.object_url("objects/abc").unwrap().as_str(),
            "https://balancehub-data.s3.us-east-1.amazonaws.com/BalanceHub/objects/abc"
        );
        let custom = store("https://s3.example.net:9000", "backup/v1");
        assert_eq!(
            custom.object_url("head.json").unwrap().as_str(),
            "https://s3.example.net:9000/balancehub-data/backup/v1/head.json"
        );
    }

    #[test]
    fn invalid_buckets_prefixes_and_keys_are_rejected() {
        assert!(S3Store::new(
            Client::new(),
            "https://s3.example.net",
            "Bad_Bucket",
            "",
            "auto",
            "key",
            "secret"
        )
        .is_err());
        assert!(normalize_prefix("a b").is_err());
        assert!(normalize_prefix("a/../b").is_err());
        assert!(normalize_prefix("sync/团队").is_err());
        assert_eq!(normalize_prefix("/sync/v1/").unwrap(), "sync/v1");
        assert!(S3Store::new(
            Client::new(),
            "",
            "balancehub-data",
            "",
            "auto",
            "key",
            "secret"
        )
        .is_err());
        let custom = store("https://s3.example.net", "");
        assert!(custom.object_url("../escape").is_err());
    }

    #[test]
    fn sigv4_matches_the_independent_reference_signature() {
        // Reference value computed with an independent Python HMAC chain for
        // GET /test.txt at 20130524T000000Z, us-east-1, empty payload.
        let store = S3Store::new(
            Client::new(),
            "https://examplebucket.s3.amazonaws.com",
            "examplebucket",
            "test.txt",
            "us-east-1",
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
        )
        .unwrap();
        let url = Url::parse("https://examplebucket.s3.amazonaws.com/test.txt").unwrap();
        let at = DateTime::parse_from_rfc3339("2013-05-24T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let headers = store.sign("GET", &url, EMPTY_SHA256, at).unwrap();
        let authorization = headers
            .iter()
            .find(|(name, _)| name == "authorization")
            .map(|(_, value)| value.as_str())
            .unwrap();
        assert_eq!(
            authorization,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=14f6a0997b2b70a86f4726658a6575b5109092ccb5fd328f51b369c44b4ac958"
        );
    }
}
