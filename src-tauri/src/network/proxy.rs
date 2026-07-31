use super::platform;
use crate::models::{AppSettings, Provider, ProviderProxyMode, ProxyMode};
use std::{
    collections::BTreeMap,
    process::Command,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const LOCAL_NO_PROXY: &str = "127.0.0.1,localhost,::1";
const PROXY_ENV_KEYS: [&str; 8] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SystemProxyConfig {
    pub http_url: String,
    pub https_url: String,
    pub all_url: String,
    pub no_proxy: String,
    /// PAC/WPAD、非 GNOME 桌面或无法可靠读取的系统配置不能静态展开。
    /// 此时 HTTP 客户端继续使用 reqwest 的系统代理能力，CLI 则保留启动环境。
    pub inherit_environment: bool,
}

impl Default for SystemProxyConfig {
    fn default() -> Self {
        Self {
            http_url: String::new(),
            https_url: String::new(),
            all_url: String::new(),
            no_proxy: String::new(),
            inherit_environment: true,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct EffectiveProxy {
    pub(super) mode: ProxyMode,
    pub(super) http_url: String,
    pub(super) https_url: String,
    pub(super) all_url: String,
    pub(super) no_proxy: String,
    pub(super) inherit_environment: bool,
}

impl EffectiveProxy {
    pub(crate) fn none() -> Self {
        Self {
            mode: ProxyMode::NoProxy,
            http_url: String::new(),
            https_url: String::new(),
            all_url: String::new(),
            no_proxy: "*".to_string(),
            inherit_environment: false,
        }
    }

    pub(crate) fn custom(url: String) -> Self {
        Self {
            mode: ProxyMode::Custom,
            http_url: String::new(),
            https_url: String::new(),
            all_url: url,
            no_proxy: LOCAL_NO_PROXY.to_string(),
            inherit_environment: false,
        }
    }

    fn system(config: SystemProxyConfig) -> Self {
        Self {
            mode: ProxyMode::System,
            http_url: config.http_url,
            https_url: config.https_url,
            all_url: config.all_url,
            no_proxy: merge_no_proxy(&config.no_proxy),
            inherit_environment: config.inherit_environment,
        }
    }

    pub(crate) fn environment(&self) -> ProxyEnvironment {
        ProxyEnvironment::from_proxy(self)
    }
}

/// 进程代理环境的唯一表示。测活命令、Unix 临时脚本和 Windows launch.json
/// 都从这里生成，避免三处各自维护一套 HTTP_PROXY/NO_PROXY 规则。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProxyEnvironment {
    inherit_environment: bool,
    remove: Vec<&'static str>,
    set: BTreeMap<&'static str, String>,
}

impl ProxyEnvironment {
    fn inherited() -> Self {
        Self {
            inherit_environment: true,
            remove: Vec::new(),
            set: BTreeMap::new(),
        }
    }

    fn from_proxy(proxy: &EffectiveProxy) -> Self {
        if proxy.inherit_environment {
            return Self::inherited();
        }

        let mut environment = Self {
            inherit_environment: false,
            remove: PROXY_ENV_KEYS.to_vec(),
            set: BTreeMap::new(),
        };
        if matches!(proxy.mode, ProxyMode::NoProxy) {
            environment.set_pair("NO_PROXY", "no_proxy", "*");
            return environment;
        }

        if matches!(proxy.mode, ProxyMode::Custom) {
            let url = proxy.all_url.trim();
            if !url.is_empty() {
                // 自定义代理在产品模型中就是单一全局代理。三组变量同时设置，兼容
                // 只识别 HTTP(S)_PROXY 或只识别 ALL_PROXY 的不同 CLI。
                environment.set_pair("HTTP_PROXY", "http_proxy", url);
                environment.set_pair("HTTPS_PROXY", "https_proxy", url);
                environment.set_pair("ALL_PROXY", "all_proxy", url);
            }
        } else {
            environment.set_pair_if_present("HTTP_PROXY", "http_proxy", &proxy.http_url);
            environment.set_pair_if_present("HTTPS_PROXY", "https_proxy", &proxy.https_url);
            environment.set_pair_if_present("ALL_PROXY", "all_proxy", &proxy.all_url);
        }

        let no_proxy = merge_no_proxy(&proxy.no_proxy);
        environment.set_pair("NO_PROXY", "no_proxy", &no_proxy);
        environment
    }

    fn set_pair(&mut self, upper: &'static str, lower: &'static str, value: &str) {
        self.set.insert(upper, value.to_string());
        self.set.insert(lower, value.to_string());
    }

    fn set_pair_if_present(&mut self, upper: &'static str, lower: &'static str, value: &str) {
        let value = value.trim();
        if !value.is_empty() {
            self.set_pair(upper, lower, value);
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn inherits(&self) -> bool {
        self.inherit_environment
    }

    pub(crate) fn removed_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.remove.iter().copied()
    }

    pub(crate) fn variables(&self) -> impl Iterator<Item = (&'static str, &str)> + '_ {
        self.set.iter().map(|(name, value)| (*name, value.as_str()))
    }

    fn apply(&self, command: &mut Command) {
        if self.inherit_environment {
            return;
        }
        for name in &self.remove {
            command.env_remove(name);
        }
        for (name, value) in &self.set {
            command.env(name, value);
        }
    }
}

pub(crate) fn resolve_proxy(settings: &AppSettings, provider: &Provider) -> EffectiveProxy {
    match provider.proxy.mode {
        ProviderProxyMode::Inherit => resolve_global_proxy(settings),
        ProviderProxyMode::System => resolve_system_proxy(),
        ProviderProxyMode::NoProxy => EffectiveProxy::none(),
        ProviderProxyMode::Custom => EffectiveProxy::custom(provider.proxy.url.clone()),
    }
}

pub(crate) fn resolve_global_proxy(settings: &AppSettings) -> EffectiveProxy {
    match settings.proxy_mode {
        ProxyMode::System => resolve_system_proxy(),
        ProxyMode::NoProxy => EffectiveProxy::none(),
        ProxyMode::Custom => EffectiveProxy::custom(settings.proxy_url.clone()),
    }
}

fn resolve_system_proxy() -> EffectiveProxy {
    // CLI 需要把手工系统代理转换成环境变量；探测结果短暂缓存，既避免每次请求
    // 拉起系统命令，又能在用户切换代理后及时刷新。
    static CACHE: OnceLock<Mutex<Option<(Instant, EffectiveProxy)>>> = OnceLock::new();
    const TTL: Duration = Duration::from_secs(10);
    let cache = CACHE.get_or_init(|| Mutex::new(None));

    if let Ok(guard) = cache.lock() {
        if let Some((resolved_at, proxy)) = guard.as_ref() {
            if resolved_at.elapsed() < TTL {
                return proxy.clone();
            }
        }
    }

    let resolved = EffectiveProxy::system(platform::system_proxy_config());
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((Instant::now(), resolved.clone()));
    }
    resolved
}

pub(crate) fn apply_proxy_env(command: &mut Command, proxy: &EffectiveProxy) {
    proxy.environment().apply(command);
}

pub(super) fn merge_no_proxy(value: &str) -> String {
    merge_no_proxy_with_local_hostname(value, local_hostname().as_deref())
}

fn merge_no_proxy_with_local_hostname(value: &str, local_hostname: Option<&str>) -> String {
    let mut entries = Vec::<String>::new();
    for entry in LOCAL_NO_PROXY
        .split(',')
        .chain(value.split([',', ';']))
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        if entry.eq_ignore_ascii_case("<local>") {
            // 标准 NO_PROXY 无法表达 Windows 的“所有无点主机名”。至少保留
            // loopback（已在默认项中）和当前机器名，且不把不受支持的标记传给 reqwest。
            if let Some(hostname) = local_hostname
                .map(str::trim)
                .filter(|host| !host.is_empty())
            {
                push_no_proxy_entry(&mut entries, hostname);
            }
            continue;
        }
        let normalized = entry
            .strip_prefix("*.")
            .map(|suffix| format!(".{suffix}"))
            .unwrap_or_else(|| entry.to_string());
        push_no_proxy_entry(&mut entries, &normalized);
    }
    entries.join(",")
}

fn push_no_proxy_entry(entries: &mut Vec<String>, entry: &str) {
    if !entries
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(entry))
    {
        entries.push(entry.to_string());
    }
}

fn local_hostname() -> Option<String> {
    ["COMPUTERNAME", "HOSTNAME"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn no_proxy_merge_normalizes_wildcards_and_local_marker() {
        assert_eq!(
            merge_no_proxy_with_local_hostname(
                "localhost;*.internal,<local>,example.com,127.0.0.1",
                Some("WORKSTATION")
            ),
            "127.0.0.1,localhost,::1,.internal,WORKSTATION,example.com"
        );
    }

    #[test]
    fn unresolved_system_proxy_preserves_inherited_cli_environment() {
        let mut command = Command::new("proxy-test");
        command.env("HTTPS_PROXY", "http://inherited:7890");
        apply_proxy_env(
            &mut command,
            &EffectiveProxy::system(SystemProxyConfig::default()),
        );

        assert_eq!(
            command_env(&command, "HTTPS_PROXY"),
            Some("http://inherited:7890")
        );
    }

    #[test]
    fn no_proxy_mode_clears_proxy_variables_and_forces_bypass() {
        let mut command = Command::new("proxy-test");
        command.env("HTTPS_PROXY", "http://inherited:7890");
        apply_proxy_env(&mut command, &EffectiveProxy::none());

        assert_eq!(command_env(&command, "HTTPS_PROXY"), None);
        assert_eq!(command_env(&command, "NO_PROXY"), Some("*"));
        assert_eq!(command_env(&command, "no_proxy"), Some("*"));
    }

    #[test]
    fn custom_socks_proxy_is_exported_for_different_cli_conventions() {
        let mut command = Command::new("proxy-test");
        apply_proxy_env(
            &mut command,
            &EffectiveProxy::custom("socks5h://127.0.0.1:1080".to_string()),
        );

        assert_eq!(
            command_env(&command, "HTTPS_PROXY"),
            Some("socks5h://127.0.0.1:1080")
        );
        assert_eq!(
            command_env(&command, "ALL_PROXY"),
            Some("socks5h://127.0.0.1:1080")
        );
        assert_eq!(
            command_env(&command, "NO_PROXY"),
            Some("127.0.0.1,localhost,::1")
        );
    }

    #[test]
    fn manual_system_proxy_keeps_protocol_specific_environment() {
        let proxy = EffectiveProxy::system(SystemProxyConfig {
            http_url: "http://127.0.0.1:7890".to_string(),
            https_url: "http://127.0.0.1:7891".to_string(),
            all_url: "socks5h://127.0.0.1:7892".to_string(),
            no_proxy: "*.internal".to_string(),
            inherit_environment: false,
        });
        let environment = proxy.environment();

        assert_eq!(
            environment_value(&environment, "HTTP_PROXY"),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(
            environment_value(&environment, "HTTPS_PROXY"),
            Some("http://127.0.0.1:7891")
        );
        assert_eq!(
            environment_value(&environment, "ALL_PROXY"),
            Some("socks5h://127.0.0.1:7892")
        );
        assert_eq!(
            environment_value(&environment, "NO_PROXY"),
            Some("127.0.0.1,localhost,::1,.internal")
        );
    }

    fn environment_value<'a>(environment: &'a ProxyEnvironment, key: &str) -> Option<&'a str> {
        environment
            .variables()
            .find_map(|(name, value)| (name == key).then_some(value))
    }

    fn command_env<'a>(command: &'a Command, key: &str) -> Option<&'a str> {
        command
            .get_envs()
            .find(|(name, _)| *name == OsStr::new(key))
            .and_then(|(_, value)| value)
            .and_then(OsStr::to_str)
    }
}
