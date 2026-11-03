//! Pinned Codex plugin MCP formats, before ordinary MCP schema decoding.
//! No commands or endpoints are executed here. Path resolution reads metadata only.

use super::super::{CodexMcpServerConfig, CodexRawMcpServerConfig};
use super::plugin::plugin_id_parts;
use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::{Component, Path, PathBuf},
};

const MCP_SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json";
const CLIENT_HEADERS: &[&str] = &[
    "accept",
    "authorization",
    "connection",
    "content-encoding",
    "content-length",
    "content-type",
    "host",
    "last-event-id",
    "mcp-protocol-version",
    "mcp-session-id",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "user-agent",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyServers {
    mcp_servers: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LegacyFile {
    Wrapped(LegacyServers),
    Bare(Map<String, Value>),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AgentFile {
    #[serde(rename = "$schema")]
    schema: String,
    mcp_servers: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum AgentServer {
    #[serde(rename = "stdio")]
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        cwd: Option<String>,
    },
    #[serde(rename = "streamable-http")]
    Http {
        url: String,
        headers: Option<BTreeMap<String, String>>,
    },
}

pub(super) fn file_servers(value: Value, agent: bool) -> Option<Map<String, Value>> {
    if agent {
        let file: AgentFile = serde_json::from_value(value).ok()?;
        (file.schema == MCP_SCHEMA).then_some(file.mcp_servers)
    } else {
        match serde_json::from_value::<LegacyFile>(value).ok()? {
            LegacyFile::Wrapped(file) => Some(file.mcp_servers),
            LegacyFile::Bare(servers) => Some(servers),
        }
    }
}

pub(super) fn normalize(
    value: Value,
    plugin_root: &Path,
    config_root: &Path,
    plugin_id: &str,
    agent: bool,
) -> Option<(toml::value::Table, CodexMcpServerConfig)> {
    let object = if agent {
        normalize_agent(value, plugin_root, &data_root(config_root, plugin_id)?)?
    } else {
        normalize_legacy(value, plugin_root)?
    };
    // JSON permits null for native Option fields. Validate it natively before
    // omitting absent values for the private TOML payload representation.
    let raw: CodexRawMcpServerConfig = serde_json::from_value(Value::Object(object)).ok()?;
    // Serialize the typed native model so ignored extension fields cannot
    // accidentally invalidate the private TOML representation.
    let mut object = serde_json::to_value(&raw).ok()?.as_object()?.clone();
    let decoded = CodexMcpServerConfig::try_from(raw).ok()?;
    if let Some(milliseconds) = object
        .get("startup_timeout_ms")
        .and_then(Value::as_u64)
        .filter(|milliseconds| *milliseconds > i64::MAX as u64)
        .filter(|_| object.get("startup_timeout_sec").is_none_or(Value::is_null))
    {
        object.remove("startup_timeout_ms");
        object.insert(
            "startup_timeout_sec".to_owned(),
            Value::from(milliseconds as f64 / 1000.0),
        );
    }
    let value = without_nulls(Value::Object(object));
    let table = toml::Value::try_from(value).ok()?.as_table()?.clone();
    Some((table, decoded))
}

fn without_nulls(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .filter(|(_, value)| !value.is_null())
                .map(|(key, value)| (key, without_nulls(value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(without_nulls).collect()),
        other => other,
    }
}

fn normalize_legacy(value: Value, root: &Path) -> Option<Map<String, Value>> {
    let mut object = value.as_object()?.clone();
    object.remove("type");
    if let Some(Value::Object(mut oauth)) = object.remove("oauth") {
        for (camel, snake) in [
            ("callbackUrl", "callback_url"),
            ("callbackPort", "callback_port"),
            ("clientId", "client_id"),
        ] {
            if let Some(value) = oauth.remove(camel) {
                oauth.entry(snake.to_owned()).or_insert(value);
            }
        }
        if !oauth.is_empty() {
            object.insert("oauth".to_owned(), Value::Object(oauth));
        }
    }
    if let Some(cwd) = object
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|cwd| !Path::new(cwd).is_absolute())
    {
        object.insert("cwd".to_owned(), Value::String(host_path(&root.join(cwd))));
    }
    Some(object)
}

fn normalize_agent(value: Value, root: &Path, data: &Path) -> Option<Map<String, Value>> {
    let object = value.as_object()?;
    let optional = match object.get("type").and_then(Value::as_str)? {
        "stdio" => "cwd",
        "streamable-http" => "headers",
        _ => return None,
    };
    if object.get(optional).is_some_and(Value::is_null) {
        return None;
    }
    match serde_json::from_value::<AgentServer>(value).ok()? {
        AgentServer::Stdio {
            command,
            args,
            env,
            cwd,
        } => normalize_stdio(command, args, env, cwd, root, data),
        AgentServer::Http { url, headers } => normalize_http(url, headers),
    }
}

fn normalize_stdio(
    mut command: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: Option<String>,
    root: &Path,
    data: &Path,
) -> Option<Map<String, Value>> {
    let bare = !command.is_empty()
        && !command.contains(['/', '\\'])
        && !matches!(
            Path::new(&command).components().next(),
            Some(Component::Prefix(_))
        );
    let relative = command.strip_prefix("./").is_some_and(portable_suffix);
    if !bare && !relative {
        return None;
    }
    let mut normalized_env = BTreeMap::new();
    for (name, value) in env {
        let name = if cfg!(windows) {
            name.to_ascii_uppercase()
        } else {
            name
        };
        if matches!(name.as_str(), "PLUGIN_ROOT" | "PLUGIN_DATA")
            || normalized_env.insert(name, value).is_some()
        {
            return None;
        }
    }
    let root = resolve_prefix(root)?;
    let data = resolve_prefix(data)?;
    let root_text = host_path(&root);
    let data_text = host_path(&data);
    if relative {
        command = host_path(&contained(&command, &root)?);
    }
    let cwd = cwd.as_deref().unwrap_or("${PLUGIN_ROOT}");
    let cwd_root = cwd_root(cwd, &root, &data)?;
    let cwd = contained(&expand(cwd, &root_text, &data_text), cwd_root)?;
    let mut normalized_env = normalized_env
        .into_iter()
        .map(|(name, value)| (name, Value::String(expand(&value, &root_text, &data_text))))
        .collect::<Map<_, _>>();
    normalized_env.insert("PLUGIN_ROOT".to_owned(), Value::String(root_text.clone()));
    normalized_env.insert("PLUGIN_DATA".to_owned(), Value::String(data_text.clone()));
    Some(Map::from_iter([
        ("command".to_owned(), Value::String(command)),
        (
            "args".to_owned(),
            Value::Array(
                args.iter()
                    .map(|arg| Value::String(expand(arg, &root_text, &data_text)))
                    .collect(),
            ),
        ),
        ("env".to_owned(), Value::Object(normalized_env)),
        ("cwd".to_owned(), Value::String(host_path(&cwd))),
    ]))
}

fn normalize_http(
    url: String,
    headers: Option<BTreeMap<String, String>>,
) -> Option<Map<String, Value>> {
    let parsed = reqwest::Url::parse(&url).ok()?;
    let host = parsed.host_str()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    let loopback = host == "localhost"
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if parsed.scheme() == "http" && !loopback {
        return None;
    }
    let mut seen = BTreeSet::new();
    let mut accepted = Map::new();
    for (name, value) in headers.unwrap_or_default() {
        if name.is_empty()
            || !seen.insert(name.to_ascii_lowercase())
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
            || value
                .bytes()
                .any(|byte| (byte < 32 && byte != b'\t') || byte == 127)
        {
            return None;
        }
        if !CLIENT_HEADERS
            .iter()
            .any(|owned| name.eq_ignore_ascii_case(owned))
        {
            accepted.insert(name, Value::String(value));
        }
    }
    let mut result = Map::from_iter([("url".to_owned(), Value::String(url))]);
    if !accepted.is_empty() {
        result.insert("http_headers".to_owned(), Value::Object(accepted));
    }
    Some(result)
}

fn data_root(config_root: &Path, id: &str) -> Option<PathBuf> {
    let (name, marketplace) = plugin_id_parts(id)?;
    let mut digest = Sha256::new();
    digest.update(marketplace.as_bytes());
    digest.update([0]);
    digest.update(name.as_bytes());
    let hash = format!("{:x}", digest.finalize());
    Some(config_root.join("plugins/data/agent-plugins").join(hash))
}

fn portable_suffix(value: &str) -> bool {
    !value.is_empty() && !value.contains('\\')
}

fn cwd_root<'a>(value: &str, root: &'a Path, data: &'a Path) -> Option<&'a Path> {
    if value == "./" || value.strip_prefix("./").is_some_and(portable_suffix) {
        return Some(root);
    }
    [("${PLUGIN_ROOT}", root), ("${PLUGIN_DATA}", data)]
        .into_iter()
        .find_map(|(prefix, root)| {
            (value == prefix
                || value
                    .strip_prefix(&format!("{prefix}/"))
                    .is_some_and(|suffix| suffix.is_empty() || portable_suffix(suffix)))
            .then_some(root)
        })
}

fn expand(value: &str, root: &str, data: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    loop {
        let next = [("${PLUGIN_ROOT}", root), ("${PLUGIN_DATA}", data)]
            .into_iter()
            .filter_map(|(pattern, replacement)| {
                remaining
                    .find(pattern)
                    .map(|offset| (offset, pattern, replacement))
            })
            .min_by_key(|(offset, _, _)| *offset);
        let Some((offset, pattern, replacement)) = next else {
            output.push_str(remaining);
            return output;
        };
        output.push_str(&remaining[..offset]);
        output.push_str(replacement);
        remaining = &remaining[offset + pattern.len()..];
    }
}

fn contained(value: &str, root: &Path) -> Option<PathBuf> {
    let path = Path::new(value);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let path = resolve_prefix(&path)?;
    path.starts_with(root).then_some(path)
}

fn resolve_prefix(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut existing = path.to_path_buf();
    let mut missing = Vec::<OsString>::new();
    loop {
        match existing.canonicalize() {
            Ok(mut path) => {
                for component in missing.iter().rev() {
                    path.push(component);
                }
                let mut normalized = PathBuf::new();
                for component in path.components() {
                    match component {
                        Component::CurDir => {}
                        Component::ParentDir => {
                            normalized.pop();
                        }
                        other => normalized.push(other.as_os_str()),
                    }
                }
                return Some(normalized);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if std::fs::symlink_metadata(&existing)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    return None;
                }
                let component = existing.components().next_back()?;
                if matches!(component, Component::Prefix(_) | Component::RootDir) {
                    return None;
                }
                missing.push(component.as_os_str().to_os_string());
                if !existing.pop() {
                    return None;
                }
            }
            Err(_) => return None,
        }
    }
}

fn host_path(path: &Path) -> String {
    let rendered = path.to_string_lossy();
    #[cfg(windows)]
    if let Some(path) = rendered.strip_prefix(r"\\?\") {
        return path
            .strip_prefix(r"UNC\")
            .map(|path| format!(r"\\{path}"))
            .unwrap_or_else(|| path.to_owned());
    }
    rendered.into_owned()
}
