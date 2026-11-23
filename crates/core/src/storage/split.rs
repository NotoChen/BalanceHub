//! 本地配置与运行时状态的分离存储。
//!
//! `config.json` 是用户可编辑的声明式配置：中转站连接方式、凭据、签到/测活
//! 设置、请求覆盖、应用设置与登录账号。`state.json` 是应用自己维护的观测
//! 结果：额度、能力探测、同步时间、签到/测活记录和失败冷却。两者按中转站
//! id 合并后仍是内存里的完整 `AppData`。
//!
//! 分离的目的不是换一种序列化格式，而是让外部编辑器修改 `config.json` 后
//! 可以在运行期安全重载，同时不会把频繁变化的额度与记录写回用户正在编辑的
//! 文件。首次升级时旧的 `data.json` 会自动拆分并保留迁移备份。

use super::file_io::{replace_data_file, DATA_FILE_NAME};
use super::validation::{normalize_provider_cached_values, validate_app_data_schema};
use crate::{
    limits,
    models::{
        AppData, AppSettings, LoginAccount, Provider, ProviderInput, ProviderStatus,
        TemporaryCliPreference, TemporaryCliProfile, Workspace, CURRENT_SCHEMA_VERSION,
    },
    util::read_text_file_limited,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};

pub(super) const CONFIG_FILE_NAME: &str = "config.json";
pub(super) const STATE_FILE_NAME: &str = "state.json";
const CONFIG_TMP_SUFFIX: &str = ".tmp";

/// 配置目录下两个文件的完整路径。`legacy` 是升级前的单文件路径，只用于
/// 迁移、恢复和提示，不再作为日常写入目标。
#[derive(Debug, Clone)]
pub(super) struct SplitPaths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    pub legacy: PathBuf,
}

pub(super) fn paths_from_data_path(data_path: &Path) -> SplitPaths {
    let root = data_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    paths_in_dir(&root)
}

pub(super) fn paths_in_dir(root: &Path) -> SplitPaths {
    SplitPaths {
        root: root.to_path_buf(),
        config: root.join(CONFIG_FILE_NAME),
        state: root.join(STATE_FILE_NAME),
        legacy: root.join(DATA_FILE_NAME),
    }
}

pub(super) fn config_tmp_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{}{CONFIG_TMP_SUFFIX}",
        path.file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default()
    ))
}

/// `config.json` 的结构：与旧 `data.json` 的 Provider 配置字段一一对应，
/// 但不再包含额度、能力探测、运行时状态和记录类字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ConfigData {
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub providers: Vec<ProviderInput>,
    #[serde(default)]
    pub settings: AppSettings,
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    #[serde(default)]
    pub temporary_cli_preferences: Vec<TemporaryCliPreference>,
    #[serde(default)]
    pub temporary_cli_profiles: Vec<TemporaryCliProfile>,
    #[serde(default)]
    pub login_accounts: Vec<LoginAccount>,
}

fn current_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

impl ConfigData {
    pub(super) fn from_app_data(data: &AppData) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            providers: data.providers.iter().map(config_provider).collect(),
            settings: data.settings.clone(),
            workspaces: data.workspaces.clone(),
            temporary_cli_preferences: data.temporary_cli_preferences.clone(),
            temporary_cli_profiles: data.temporary_cli_profiles.clone(),
            login_accounts: data.login_accounts.clone(),
        }
    }

    /// 读取 `config.json` 时允许缺省字段（用户手写常见），但中转站必须有
    /// 稳定 id，否则无法与 `state.json` 中的运行时状态对应。
    ///
    /// 站点地址不在这里强制：卡片允许先建后填，历史数据和手写配置都可能
    /// 暂时没有地址。缺地址只会让该中转站显示为不可用，不应该让整份配置
    /// 读取失败，更不能让迁移后的用户无法再启动。
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(format!(
                "config.json 结构版本不兼容：当前应用支持 schemaVersion {CURRENT_SCHEMA_VERSION}，检测到 {}",
                self.schema_version
            ));
        }
        let mut ids = HashSet::new();
        for input in &self.providers {
            let id = input.id.as_deref().unwrap_or("").trim();
            if id.is_empty() {
                return Err("config.json 中存在缺少 id 的中转站".to_string());
            }
            if id.len() > 256 || id.chars().any(char::is_control) {
                return Err("config.json 中存在无效的中转站 id".to_string());
            }
            if !ids.insert(id.to_string()) {
                return Err(format!("config.json 中存在重复的中转站 id：{id}"));
            }
        }
        Ok(())
    }

    /// 以当前内存状态为运行时基底应用配置。`apply_input` 与编辑卡片走同一
    /// 套规则：换站点/换凭据时清理旧额度与探测结果，改请求覆盖时重置失败
    /// 冷却；未出现在配置里的中转站会被删除。
    pub(super) fn into_app_data(self, current: &AppData) -> Result<AppData, String> {
        self.validate()?;
        let existing: HashMap<&str, &Provider> = current
            .providers
            .iter()
            .map(|provider| (provider.identity.id.as_str(), provider))
            .collect();
        let mut providers = Vec::with_capacity(self.providers.len());
        for mut input in self.providers {
            let id = input
                .id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| "config.json 中存在缺少 id 的中转站".to_string())?
                .to_string();
            // `ProviderInput` 里 `agent_models: None` 表示「编辑器没有携带
            // 该字段，保留现状」。配置文件是完整的声明式状态，缺省应视为
            // 清空，否则手写配置删掉绑定后永远无法生效。
            input.cli.agent_models.get_or_insert_with(Default::default);
            let mut provider = match existing.get(id.as_str()) {
                Some(previous) => (*previous).clone(),
                None => Provider::from_input(ProviderInput::default(), id.clone()),
            };
            provider.apply_input(input);
            provider.revision = 0;
            providers.push(provider);
        }
        Ok(AppData {
            revision: 0,
            schema_version: CURRENT_SCHEMA_VERSION,
            providers,
            settings: self.settings,
            workspaces: self.workspaces,
            temporary_cli_preferences: self.temporary_cli_preferences,
            temporary_cli_profiles: self.temporary_cli_profiles,
            login_accounts: self.login_accounts,
        })
    }
}

fn config_provider(provider: &Provider) -> ProviderInput {
    ProviderInput {
        id: Some(provider.identity.id.clone()),
        identity: crate::models::ProviderIdentityInput {
            name: provider.identity.name.clone(),
            base_url: provider.identity.base_url.clone(),
            protocol: provider.identity.protocol,
            remark: provider.identity.remark.clone(),
            user_id: provider.identity.user_id.clone(),
            backup_urls: provider.identity.backup_urls.clone(),
        },
        auth: provider.auth.clone(),
        cli: crate::models::ProviderCliInput {
            preferred_model: provider.cli.preferred_model.clone(),
            agent_models: Some(provider.cli.agent_models.clone()),
        },
        automation: crate::models::ProviderAutomationInput {
            refresh_interval: provider.automation.refresh_interval,
            check_in_time: provider.automation.check_in_time.clone(),
            check_in_method: provider.automation.check_in_method,
            auto_shield: provider.automation.auto_shield,
            turnstile_mode: provider.automation.turnstile_mode,
        },
        liveness: crate::models::ProviderLivenessInput {
            use_global: provider.liveness.use_global,
            enabled: provider.liveness.enabled,
            agent_base_urls: provider.liveness.agent_base_urls.clone(),
            cli_kind: provider.liveness.cli_kind,
            interval_mode: provider.liveness.interval_mode,
            interval: provider.liveness.interval,
            random_min_interval: provider.liveness.random_min_interval,
            random_max_interval: provider.liveness.random_max_interval,
            timeout: provider.liveness.timeout,
            model: provider.liveness.model.clone(),
            prompt_mode: provider.liveness.prompt_mode,
            fixed_prompt: provider.liveness.fixed_prompt.clone(),
        },
        proxy: provider.proxy.clone(),
        notification: provider.notification.clone(),
        runtime: crate::models::ProviderRuntimeInput {
            enabled: provider.runtime.enabled,
        },
        request: provider.request.clone(),
        quota_spec: provider.quota_spec.clone(),
        model_config: provider.model_config.clone(),
    }
}

/// `state.json` 的结构：全部字段都是应用观测/调度结果，按中转站 id 覆盖到
/// 配置之上。缺少某个中转站的 state 时按新卡片处理，不影响配置加载。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StateData {
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub providers: Vec<ProviderState>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct ProviderState {
    pub id: String,
    pub identity: CachedIdentity,
    pub quota: Option<crate::models::ProviderQuota>,
    pub capabilities: Option<crate::models::ProviderCapabilities>,
    pub automation: Option<AutomationState>,
    pub liveness: Option<LivenessState>,
    pub runtime: Option<RuntimeState>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct CachedIdentity {
    pub display_name: String,
    pub username: String,
    pub site_logo: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct AutomationState {
    pub last_synced_at: Option<String>,
    pub last_checked_in_at: Option<String>,
    pub last_check_in_user: String,
    pub check_in_records: Vec<crate::models::ProviderCheckInRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct LivenessState {
    pub prompt_cursor: u64,
    pub next_at: Option<String>,
    pub records: Vec<crate::models::LivenessRecord>,
    pub run_count: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    pub total_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RuntimeState {
    pub status: ProviderStatus,
    pub error_message: Option<String>,
    #[serde(default)]
    pub consecutive_failures: u32,
    #[serde(default)]
    pub cooldown_until: Option<i64>,
    #[serde(default)]
    pub cooldown_reason: Option<String>,
    #[serde(default)]
    pub credential_rejected: bool,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            status: ProviderStatus::Warning,
            error_message: Some("尚未同步".to_string()),
            consecutive_failures: 0,
            cooldown_until: None,
            cooldown_reason: None,
            credential_rejected: false,
        }
    }
}

impl StateData {
    pub(super) fn from_app_data(data: &AppData) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            providers: data
                .providers
                .iter()
                .map(ProviderState::from_provider)
                .collect(),
        }
    }

    pub(super) fn apply_to(&self, data: &mut AppData) {
        let states: HashMap<&str, &ProviderState> = self
            .providers
            .iter()
            .map(|state| (state.id.as_str(), state))
            .collect();
        for provider in &mut data.providers {
            let id = provider.identity.id.clone();
            let defaults = Provider::from_input(ProviderInput::default(), id.clone());
            match states.get(id.as_str()) {
                Some(state) => state.apply_to(provider, &defaults),
                None => apply_default_state(provider, &defaults),
            }
        }
    }
}

impl ProviderState {
    fn from_provider(provider: &Provider) -> Self {
        Self {
            id: provider.identity.id.clone(),
            identity: CachedIdentity {
                display_name: provider.identity.display_name.clone(),
                username: provider.identity.username.clone(),
                site_logo: provider.identity.site_logo.clone(),
            },
            quota: Some(provider.quota.clone()),
            capabilities: Some(provider.capabilities.clone()),
            automation: Some(AutomationState {
                last_synced_at: provider.automation.last_synced_at.clone(),
                last_checked_in_at: provider.automation.last_checked_in_at.clone(),
                last_check_in_user: provider.automation.last_check_in_user.clone(),
                check_in_records: provider.automation.check_in_records.clone(),
            }),
            liveness: Some(LivenessState {
                prompt_cursor: provider.liveness.prompt_cursor,
                next_at: provider.liveness.next_at.clone(),
                records: provider.liveness.records.clone(),
                run_count: provider.liveness.run_count,
                total_input_tokens: provider.liveness.total_input_tokens,
                total_output_tokens: provider.liveness.total_output_tokens,
                total_tokens: provider.liveness.total_tokens,
                total_cost_usd: provider.liveness.total_cost_usd,
            }),
            runtime: Some(RuntimeState {
                status: provider.runtime.status,
                error_message: provider.runtime.error_message.clone(),
                consecutive_failures: provider.runtime.consecutive_failures,
                cooldown_until: provider.runtime.cooldown_until,
                cooldown_reason: provider.runtime.cooldown_reason.clone(),
                credential_rejected: provider.runtime.credential_rejected,
            }),
        }
    }

    fn apply_to(&self, provider: &mut Provider, defaults: &Provider) {
        provider.identity.display_name = self.identity.display_name.clone();
        provider.identity.username = self.identity.username.clone();
        provider.identity.site_logo = self.identity.site_logo.clone();
        provider.quota = self.quota.clone().unwrap_or_else(|| defaults.quota.clone());
        provider.capabilities = self
            .capabilities
            .clone()
            .unwrap_or_else(|| defaults.capabilities.clone());
        match &self.automation {
            Some(state) => {
                provider.automation.last_synced_at = state.last_synced_at.clone();
                provider.automation.last_checked_in_at = state.last_checked_in_at.clone();
                provider.automation.last_check_in_user = state.last_check_in_user.clone();
                provider.automation.check_in_records = state.check_in_records.clone();
            }
            None => {
                provider.automation.last_synced_at = None;
                provider.automation.last_checked_in_at = None;
                provider.automation.last_check_in_user.clear();
                provider.automation.check_in_records.clear();
            }
        }
        match &self.liveness {
            Some(state) => {
                provider.liveness.prompt_cursor = state.prompt_cursor;
                provider.liveness.next_at = state.next_at.clone();
                provider.liveness.records = state.records.clone();
                provider.liveness.run_count = state.run_count;
                provider.liveness.total_input_tokens = state.total_input_tokens;
                provider.liveness.total_output_tokens = state.total_output_tokens;
                provider.liveness.total_tokens = state.total_tokens;
                provider.liveness.total_cost_usd = state.total_cost_usd;
            }
            None => {
                provider.liveness.prompt_cursor = 0;
                provider.liveness.next_at = None;
                provider.liveness.records.clear();
                provider.liveness.run_count = 0;
                provider.liveness.total_input_tokens = 0;
                provider.liveness.total_output_tokens = 0;
                provider.liveness.total_tokens = 0;
                provider.liveness.total_cost_usd = 0.0;
            }
        }
        match &self.runtime {
            Some(state) => {
                provider.runtime.status = state.status;
                provider.runtime.error_message = state.error_message.clone();
                provider.runtime.consecutive_failures = state.consecutive_failures;
                provider.runtime.cooldown_until = state.cooldown_until;
                provider.runtime.cooldown_reason = state.cooldown_reason.clone();
                provider.runtime.credential_rejected = state.credential_rejected;
            }
            None => {
                provider.runtime.status = defaults.runtime.status;
                provider.runtime.error_message = defaults.runtime.error_message.clone();
                provider.runtime.consecutive_failures = 0;
                provider.runtime.cooldown_until = None;
                provider.runtime.cooldown_reason = None;
                provider.runtime.credential_rejected = false;
            }
        }
    }
}

fn apply_default_state(provider: &mut Provider, defaults: &Provider) {
    provider.identity.display_name.clear();
    provider.identity.username.clear();
    provider.identity.site_logo.clear();
    provider.quota = defaults.quota.clone();
    provider.capabilities = defaults.capabilities.clone();
    provider.automation.last_synced_at = None;
    provider.automation.last_checked_in_at = None;
    provider.automation.last_check_in_user.clear();
    provider.automation.check_in_records.clear();
    provider.liveness.prompt_cursor = 0;
    provider.liveness.next_at = None;
    provider.liveness.records.clear();
    provider.liveness.run_count = 0;
    provider.liveness.total_input_tokens = 0;
    provider.liveness.total_output_tokens = 0;
    provider.liveness.total_tokens = 0;
    provider.liveness.total_cost_usd = 0.0;
    provider.runtime.status = defaults.runtime.status;
    provider.runtime.error_message = defaults.runtime.error_message.clone();
    provider.runtime.consecutive_failures = 0;
    provider.runtime.cooldown_until = None;
    provider.runtime.cooldown_reason = None;
    provider.runtime.credential_rejected = false;
}

/// 把配置与状态合并成一份完整、已校验的 `AppData`。冷启动时 `current`
/// 是空数据；热重载时传入内存快照，运行期字段优先保留内存中的最新值。
pub(super) fn merge_config(
    current: &AppData,
    config: ConfigData,
    state: Option<StateData>,
) -> Result<(AppData, bool), String> {
    let mut data = config.into_app_data(current)?;
    if let Some(state) = state {
        state.apply_to(&mut data);
    }
    validate_app_data_schema(&data)?;
    let mut normalized = normalize_provider_cached_values(&mut data);
    normalized |= limits::normalize_app_data(&mut data);
    validate_app_data_schema(&data)?;
    Ok((data, normalized))
}

/// 把完整状态投影成配置。用于比较外部配置是否真的带来了变化，也用于
/// 生成 `config.json`。
pub(super) fn config_bytes(data: &AppData) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&ConfigData::from_app_data(data))
        .map_err(|error| format!("序列化 config.json 失败: {error}"))
}

/// 读取并校验 `config.json`。解析/版本错误会记下当前文件指纹，让启动失败
/// 后的自动重试和运行期监听只在文件再次变化时才重读，而不是每秒刷屏。
///
/// 读取成功会立即刷新缓存，并把「这份内容对应哪个文件版本」固定下来。
/// 编辑器在读取窗口内又保存了一次时，缓存指纹会被置空，监听线程下一轮
/// 重新读取；期间任何保存都会先核对磁盘，不会覆盖用户刚写下的内容。
pub(super) fn read_config_file(path: &Path) -> Result<ConfigData, String> {
    let (text, stable_fingerprint) = read_config_text(path)?;
    // 用户会在 Windows 编辑器里直接改配置；UTF-8 BOM 不是合法 JSON，
    // 但几乎总是想表达同一份文件，读取时剥掉而不是报解析失败。
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let parsed = (|| {
        let value = serde_json::from_str::<serde_json::Value>(text)
            .map_err(|error| format!("解析 config.json 失败({}): {error}", path.display()))?;
        let stored_version = value
            .get("schemaVersion")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(u64::from(CURRENT_SCHEMA_VERSION)) as u32;
        if stored_version > CURRENT_SCHEMA_VERSION {
            return Err(format!(
                "config.json 结构版本过新：当前应用支持 schemaVersion {CURRENT_SCHEMA_VERSION}，检测到 {stored_version}；请升级应用"
            ));
        }
        if stored_version < CURRENT_SCHEMA_VERSION {
            // 拆分存储从当前 schema 开始；旧版本迁移仍走 data.json 导入。
            return Err(format!(
                "config.json 结构版本较旧（{stored_version}）；请通过「导入配置」使用旧版 data.json 完成迁移"
            ));
        }
        let mut config = serde_json::from_value::<ConfigData>(value)
            .map_err(|error| format!("解析 config.json 失败({}): {error}", path.display()))?;
        config.schema_version = CURRENT_SCHEMA_VERSION;
        config.validate()?;
        Ok(config)
    })();
    match parsed {
        Ok(config) => {
            let canonical = serde_json::to_vec(&config)
                .map_err(|error| format!("序列化 config.json 失败: {error}"))?;
            note_config_read(path, canonical, stable_fingerprint);
            Ok(config)
        }
        Err(error) => {
            note_config_error(path);
            Err(error)
        }
    }
}

/// 编辑器保存可能发生在读取过程中；读到一半的文件不能当成最终配置。
/// 先取指纹再读、读完复核，不一致就短暂重读。超过时限只交出最后一次
/// 读取结果，同时返回 `None`，让调用方把缓存标记为需要再次加载。
fn read_config_text(path: &Path) -> Result<(String, Option<Fingerprint>), String> {
    let deadline = Instant::now() + Duration::from_millis(1200);
    loop {
        let before = fingerprint(path);
        let text = match read_text_file_limited(path, limits::MAX_CONFIG_FILE_BYTES, "读取配置")
        {
            Ok(text) => text,
            Err(error) => {
                note_config_error(path);
                return Err(error);
            }
        };
        let after = fingerprint(path);
        if before == after {
            return Ok((text, after));
        }
        if Instant::now() >= deadline {
            return Ok((text, None));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// 运行时状态损坏不应阻止应用启动；重置成默认状态比带着半份额度数据运行
/// 更安全。文件本身保留，下一次保存会覆盖。
pub(super) fn read_state_file(path: &Path) -> (Option<StateData>, Option<String>) {
    if !path.exists() {
        return (None, None);
    }
    let text = match read_text_file_limited(path, limits::MAX_STATE_FILE_BYTES, "读取运行状态")
    {
        Ok(text) => text,
        Err(error) => return (None, Some(error)),
    };
    let value = match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(value) => value,
        Err(error) => {
            return (
                None,
                Some(format!("解析 state.json 失败({}): {error}", path.display())),
            )
        }
    };
    let stored_version = value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
    if stored_version != CURRENT_SCHEMA_VERSION {
        return (
            None,
            Some(format!(
                "state.json 结构版本不匹配（{stored_version}），已按默认运行状态启动"
            )),
        );
    }
    match serde_json::from_value::<StateData>(value) {
        Ok(state) => (Some(state), None),
        Err(error) => (
            None,
            Some(format!("解析 state.json 失败({}): {error}", path.display())),
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
    len: u64,
    modified: Option<SystemTime>,
}

fn fingerprint(path: &Path) -> Option<Fingerprint> {
    let metadata = fs::metadata(path).ok()?;
    Some(Fingerprint {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

#[derive(Debug, Clone)]
struct ConfigCache {
    fingerprint: Option<Fingerprint>,
    /// `config.json` 上次被成功读取/写出时的规范化语义内容。
    canonical: Option<Vec<u8>>,
    /// 外部文件当前无法解析或已被删除时保持 false，阻止后续保存覆盖它。
    readable: bool,
    /// 加载时文件本就不存在。只有这种状态允许第一次保存创建文件。
    may_create: bool,
}

#[derive(Debug, Clone, Default)]
struct StateCache {
    fingerprint: Option<Fingerprint>,
    canonical: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default)]
struct CacheEntry {
    config: Option<ConfigCache>,
    state: StateCache,
}

static CACHE: OnceLock<Mutex<HashMap<PathBuf, CacheEntry>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<PathBuf, CacheEntry>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with_cache<R>(root: &Path, action: impl FnOnce(&mut CacheEntry) -> R) -> R {
    let mut entries = cache().lock().unwrap_or_else(|error| error.into_inner());
    action(entries.entry(root.to_path_buf()).or_default())
}

/// 缓存按配置目录共享；只有真正的 `config.json` 才能代表该目录的当前
/// 配置。读取备份、导出文件或任意 JSON 时不能污染主配置的缓存状态。
fn is_live_config(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some(CONFIG_FILE_NAME)
}

/// 读取成功后刷新缓存。`stable` 是读取窗口结束时的文件版本；为 `None`
/// 说明读取期间文件仍在变化，缓存内容可以先用，但指纹留空，监听线程
/// 下一轮会重新读取并在必要时覆盖内存。
fn note_config_read(path: &Path, canonical: Vec<u8>, stable: Option<Fingerprint>) {
    if !is_live_config(path) {
        return;
    }
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    let current = fingerprint(path);
    // 读取到最后一次复核之间仍有极短窗口；再核对一次，避免把旧内容
    // 标成用户的最新版本。
    let fingerprint = match stable {
        Some(expected) if current.as_ref() == Some(&expected) => current,
        _ => None,
    };
    with_cache(root, |entry| {
        entry.config = Some(ConfigCache {
            fingerprint,
            canonical: Some(canonical),
            readable: true,
            may_create: false,
        });
    });
}

/// 热重载合并后刷新缓存的规范化内容。这里不改指纹，只在磁盘版本仍等于
/// 读取时版本的情况下保留它；否则置空等待下一轮重读。保存路径据此先核对
/// 磁盘，不会把读取期间发生的外部编辑覆盖掉。
fn note_config_canonical(paths: &SplitPaths, canonical: Vec<u8>) {
    let current = fingerprint(&paths.config);
    with_cache(&paths.root, |entry| {
        let stable = entry.config.as_ref().is_some_and(|cache| {
            cache.readable && cache.fingerprint.is_some() && cache.fingerprint == current
        });
        entry.config = Some(ConfigCache {
            fingerprint: if stable { current } else { None },
            canonical: Some(canonical),
            readable: true,
            may_create: false,
        });
    });
}

/// 保存时确认磁盘已被外部改写、且本机修改尚未落盘。内存没有采纳磁盘
/// 版本，所以不能把磁盘标成“已加载”：把指纹置空，监听线程下一轮重新
/// 读取并应用外部配置；在那之前任何保存都会继续先核对磁盘。
fn defer_config_reload(root: &Path, canonical: Vec<u8>) {
    with_cache(root, |entry| {
        entry.config = Some(ConfigCache {
            fingerprint: None,
            canonical: Some(canonical),
            readable: true,
            may_create: false,
        });
    });
}

fn note_config_error(path: &Path) {
    if !is_live_config(path) {
        return;
    }
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    let fingerprint = fingerprint(path);
    with_cache(root, |entry| {
        entry.config = Some(ConfigCache {
            fingerprint,
            canonical: None,
            readable: false,
            may_create: false,
        });
    });
}

fn note_state_loaded(root: &Path, path: &Path, state: &StateData) {
    let fingerprint = fingerprint(path);
    let canonical = serde_json::to_vec(state).ok();
    with_cache(root, |entry| {
        entry.state = StateCache {
            fingerprint,
            canonical,
        };
    });
}

/// 监听线程用的廉价检查：只看指纹，不读文件内容。
pub(super) fn config_file_changed(paths: &SplitPaths) -> bool {
    let current = fingerprint(&paths.config);
    with_cache(&paths.root, |entry| match &entry.config {
        None => true,
        Some(cache) => cache.fingerprint != current,
    })
}

/// 启动读取失败时用于判断「用户是否真的改过文件」。配置目录已经有拆分
/// 文件时看 config.json/state.json；仍停留在旧版单文件时看 data.json。
/// 返回值的格式只需在当前进程内可比较，不参与持久化。
pub(super) fn retry_signature(paths: &SplitPaths) -> String {
    let files: Vec<&Path> = if paths.config.exists() || paths.state.exists() {
        vec![&paths.config, &paths.state]
    } else {
        vec![&paths.legacy]
    };
    files
        .iter()
        .map(|path| match fingerprint(path) {
            Some(value) => format!("{}:{}:{:?}", path.display(), value.len, value.modified),
            None => format!("{}:-", path.display()),
        })
        .collect::<Vec<_>>()
        .join("|")
}

pub(super) fn mark_config_missing(root: &Path) {
    with_cache(root, |entry| {
        entry.config = Some(ConfigCache {
            fingerprint: None,
            canonical: None,
            readable: false,
            may_create: false,
        });
    });
}

pub(super) fn mark_config_fresh(root: &Path) {
    with_cache(root, |entry| {
        entry.config = Some(ConfigCache {
            fingerprint: None,
            canonical: None,
            readable: true,
            may_create: true,
        });
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SaveMode {
    /// 普通保存：外部已经修改/损坏的 `config.json` 绝不能被覆盖。
    RespectExternalEdits,
    /// 导入、自动恢复等用户明确要求整体替换的入口。
    Replace,
}

pub(super) fn save_split(paths: &SplitPaths, data: &AppData, mode: SaveMode) -> Result<(), String> {
    let config = ConfigData::from_app_data(data);
    let state = StateData::from_app_data(data);
    let config_value =
        serde_json::to_vec(&config).map_err(|error| format!("序列化 config.json 失败: {error}"))?;
    let state_value =
        serde_json::to_vec(&state).map_err(|error| format!("序列化 state.json 失败: {error}"))?;

    if let Some(parent) = paths.config.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("创建配置目录失败({}): {error}", parent.display()))?;
    }

    let current_config_fingerprint = fingerprint(&paths.config);
    let (cached_config, cached_state) = with_cache(&paths.root, |entry| {
        (entry.config.clone(), entry.state.clone())
    });
    let mut write_config = false;

    match mode {
        SaveMode::Replace => {
            write_config = true;
        }
        SaveMode::RespectExternalEdits => match cached_config {
            None => {
                if current_config_fingerprint.is_some() {
                    // 没有加载记录却已有文件：先读出来判断，不能盲目覆盖。
                    // `read_config_file` 会同时刷新缓存指纹。
                    let disk = read_config_file(&paths.config)?;
                    let disk_value = serde_json::to_vec(&disk)
                        .map_err(|error| format!("序列化 config.json 失败: {error}"))?;
                    if disk_value != config_value {
                        defer_config_reload(&paths.root, disk_value);
                        return Err(config_conflict_error(&paths.config));
                    }
                } else {
                    write_config = true;
                }
            }
            Some(cache) if !cache.readable => {
                if current_config_fingerprint != cache.fingerprint {
                    return Err(format!(
                        "{} 已在外部发生变化，请等待重新加载后再保存",
                        paths.config.display()
                    ));
                }
                return Err(format!(
                    "{} 当前无法解析或已被删除；为避免覆盖外部文件，本次修改尚未保存",
                    paths.config.display()
                ));
            }
            Some(cache) if current_config_fingerprint != cache.fingerprint => {
                match current_config_fingerprint {
                    None if cache.may_create => {
                        write_config = true;
                    }
                    None => {
                        return Err(format!(
                            "{} 已被外部删除；为避免误建配置，本次修改尚未保存",
                            paths.config.display()
                        ));
                    }
                    Some(_) => {
                        // 外部文件已变化：读出来与本机待保存内容做语义比较，
                        // 同时让 `read_config_file` 刷新缓存指纹。
                        let disk = read_config_file(&paths.config)?;
                        let disk_value = serde_json::to_vec(&disk)
                            .map_err(|error| format!("序列化 config.json 失败: {error}"))?;
                        if disk_value != config_value {
                            defer_config_reload(&paths.root, disk_value);
                            return Err(config_conflict_error(&paths.config));
                        }
                    }
                }
            }
            Some(cache) => {
                // 文件自上次读取/写出后没有变化：允许写入本地修改和规范化结果。
                write_config = cache.canonical.as_deref() != Some(config_value.as_slice());
            }
        },
    }

    // 先写运行状态，再写用户配置。配置替换失败时，内存尚未提交，state.json
    // 最多多一份下一次启动会重新合并的观测结果；反过来则会出现磁盘配置已
    // 更新、调用方却收到失败的情况。
    let write_state = cached_state.fingerprint != fingerprint(&paths.state)
        || cached_state.canonical.as_deref() != Some(state_value.as_slice());
    if write_state {
        write_state_file(paths, &state)?;
        with_cache(&paths.root, |entry| {
            entry.state = StateCache {
                fingerprint: fingerprint(&paths.state),
                canonical: Some(state_value),
            };
        });
    }

    if write_config {
        write_config_file(paths, &config, mode)?;
        with_cache(&paths.root, |entry| {
            entry.config = Some(ConfigCache {
                fingerprint: fingerprint(&paths.config),
                canonical: Some(config_value),
                readable: true,
                may_create: false,
            });
        });
    }
    Ok(())
}

fn write_state_file(paths: &SplitPaths, state: &StateData) -> Result<(), String> {
    let tmp = config_tmp_path(&paths.state);
    super::file_io::write_json_file_limited(
        &tmp,
        state,
        limits::MAX_STATE_FILE_BYTES,
        "保存运行状态",
    )?;
    replace_data_file(&tmp, &paths.state)
}

fn write_config_file(
    paths: &SplitPaths,
    config: &ConfigData,
    mode: SaveMode,
) -> Result<(), String> {
    let tmp = config_tmp_path(&paths.config);
    super::file_io::write_json_file_limited(
        &tmp,
        config,
        limits::MAX_CONFIG_FILE_BYTES,
        "保存配置",
    )?;
    if mode == SaveMode::RespectExternalEdits {
        // 写临时文件期间外部编辑器可能刚好完成一次保存；替换前再核对一次
        // 指纹，宁可放弃这次配置写入，也不覆盖用户刚写下的内容。
        let expected = with_cache(&paths.root, |entry| {
            entry
                .config
                .as_ref()
                .and_then(|cache| cache.fingerprint.clone())
        });
        if fingerprint(&paths.config) != expected {
            let _ = fs::remove_file(&tmp);
            return Err(format!(
                "{} 在保存期间再次被修改，本次修改尚未保存；请等待重新加载后重试",
                paths.config.display()
            ));
        }
    }
    replace_data_file(&tmp, &paths.config)
}

fn config_conflict_error(path: &Path) -> String {
    format!(
        "{} 已在外部修改，当前修改尚未保存。请等待界面自动重新加载后再试；外部编辑器尚未保存时会先保留你的文件。",
        path.display()
    )
}

pub(super) fn save_state_only(paths: &SplitPaths, data: &AppData) -> Result<(), String> {
    let state = StateData::from_app_data(data);
    let value =
        serde_json::to_vec(&state).map_err(|error| format!("序列化 state.json 失败: {error}"))?;
    let current_fingerprint = fingerprint(&paths.state);
    let (write, _) = with_cache(&paths.root, |entry| {
        let write = entry.state.fingerprint != current_fingerprint
            || entry.state.canonical.as_deref() != Some(value.as_slice());
        (write, ())
    });
    if !write {
        return Ok(());
    }
    write_state_file(paths, &state)?;
    with_cache(&paths.root, |entry| {
        entry.state = StateCache {
            fingerprint: fingerprint(&paths.state),
            canonical: Some(value),
        };
    });
    Ok(())
}

/// 以配置为基准重建内存数据，保留 `current` 中尚未落盘的运行期字段。
pub(super) fn reload_data_from_files(
    paths: &SplitPaths,
    current: &AppData,
    config: ConfigData,
) -> Result<AppData, String> {
    let (next, _) = merge_config(current, config, None)?;
    note_config_canonical(
        paths,
        serde_json::to_vec(&ConfigData::from_app_data(&next))
            .map_err(|error| format!("序列化 config.json 失败: {error}"))?,
    );
    Ok(next)
}

/// 冷启动读取拆分文件；`state.json` 缺失或损坏时只重置运行期字段。
pub(super) fn load_data_from_files(
    paths: &SplitPaths,
    current: &AppData,
) -> Result<(AppData, bool, Option<String>), String> {
    let config = read_config_file(&paths.config)?;
    let (state, state_warning) = read_state_file(&paths.state);
    if let Some(state) = &state {
        note_state_loaded(&paths.root, &paths.state, state);
    }
    let (data, normalized) = merge_config(current, config, state)?;
    Ok((data, normalized, state_warning))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProviderInput, ProviderProtocol};

    fn fixture() -> AppData {
        let mut input = ProviderInput::default();
        input.identity.name = "Fixture".into();
        input.identity.base_url = "https://fixture.example".into();
        input.identity.protocol = ProviderProtocol::NewApi;
        input.auth.mode = crate::models::AuthMode::Session;
        input.auth.session_cookie = "session=fixture".into();
        input.auth.api_user = "42".into();
        input.automation.check_in_method = crate::models::ProviderCheckInMethod::SessionSignIn;
        input.request.endpoints = vec![
            crate::models::provider_domain::endpoints::ProviderEndpointOverride {
                operation: "check_in".into(),
                path: "/api/user/daily-reward".into(),
                query: Vec::new(),
                headers: Vec::new(),
            },
        ];
        let mut provider = Provider::from_input(input, "fixture-provider".into());
        provider.quota.available = 12.5;
        provider.quota.used = 3.5;
        provider.capabilities.check_in_known = true;
        provider.capabilities.check_in_supported = true;
        provider.automation.last_synced_at = Some("1700000000".into());
        provider
            .automation
            .check_in_records
            .push(crate::models::ProviderCheckInRecord {
                date: "2023-11-14".into(),
                checked_at: Some("1700000000".into()),
                quota_delta: Some(1.0),
                message: "ok".into(),
            });
        provider.liveness.next_at = Some("1800000000".into());
        provider.runtime.status = ProviderStatus::Ok;
        provider.runtime.error_message = None;
        provider
            .cli
            .agent_models
            .insert("codex".into(), "gpt-5".into());
        AppData::new_current(vec![provider], AppSettings::default())
    }

    fn temp_paths() -> (tempfile::TempDir, SplitPaths) {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths_in_dir(directory.path());
        (directory, paths)
    }

    #[test]
    fn config_projection_omits_runtime_and_keeps_agent_bindings() {
        let data = fixture();
        let config = ConfigData::from_app_data(&data);
        let value = serde_json::to_value(&config).unwrap();
        let provider = &value["providers"][0];
        assert_eq!(provider["id"], "fixture-provider");
        assert_eq!(
            provider["request"]["endpoints"][0]["path"],
            "/api/user/daily-reward"
        );
        assert_eq!(provider["cli"]["agentModels"]["codex"], "gpt-5");
        assert!(provider.get("quota").is_none());
        assert!(provider.get("capabilities").is_none());
        // runtime 只保留「是否启用」这类配置开关，运行状态不进 config.json。
        assert_eq!(provider["runtime"]["enabled"], true);
        assert!(provider["runtime"].get("status").is_none());
        assert!(provider["runtime"].get("lastError").is_none());
        assert!(provider["automation"].get("lastSyncedAt").is_none());
        assert!(provider["liveness"].get("nextAt").is_none());
    }

    #[test]
    fn state_round_trip_preserves_all_runtime_fields() {
        let data = fixture();
        let state = StateData::from_app_data(&data);
        let mut target = ConfigData::from_app_data(&data)
            .into_app_data(&AppData::default())
            .unwrap();
        state.apply_to(&mut target);
        assert_eq!(target.providers[0].quota.available, 12.5);
        assert_eq!(target.providers[0].quota.used, 3.5);
        assert!(target.providers[0].capabilities.check_in_supported);
        assert_eq!(
            target.providers[0].automation.last_synced_at.as_deref(),
            Some("1700000000")
        );
        assert_eq!(target.providers[0].automation.check_in_records.len(), 1);
        assert_eq!(
            target.providers[0].liveness.next_at.as_deref(),
            Some("1800000000")
        );
        assert!(matches!(
            target.providers[0].runtime.status,
            ProviderStatus::Ok
        ));
        assert!(target.providers[0].runtime.error_message.is_none());
        assert_eq!(target.providers[0].cli.agent_models["codex"], "gpt-5");
        // 配置字段从 config 侧恢复。
        assert_eq!(
            target.providers[0].request.endpoints[0].path,
            "/api/user/daily-reward"
        );
    }

    #[test]
    fn save_split_writes_both_files_and_runtime_save_does_not_rewrite_config() {
        let (_directory, paths) = temp_paths();
        let mut data = fixture();
        save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap();
        assert!(paths.config.exists());
        assert!(paths.state.exists());
        let config_before = fs::read(&paths.config).unwrap();

        data.providers[0].quota.available = 99.0;
        data.providers[0].automation.last_synced_at = Some("1700009999".into());
        save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap();
        assert_eq!(fs::read(&paths.config).unwrap(), config_before);

        let loaded = load_data_from_files(&paths, &AppData::default()).unwrap().0;
        assert_eq!(loaded.providers[0].quota.available, 99.0);
        assert_eq!(
            loaded.providers[0].automation.last_synced_at.as_deref(),
            Some("1700009999")
        );
    }

    #[test]
    fn external_config_change_is_only_applied_after_reload_and_runtime_is_kept() {
        let (_directory, paths) = temp_paths();
        let data = fixture();
        save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap();

        let mut config = read_config_file(&paths.config).unwrap();
        config.providers[0].identity.name = "Renamed".into();
        config.providers[0].request.endpoints[0].path = "/api/user/renamed".into();
        write_config_file(&paths, &config, SaveMode::Replace).unwrap();
        // 模拟外部编辑器写入：清除缓存指纹，让监听确认文件已变化。
        with_cache(&paths.root, |entry| {
            if let Some(cache) = &mut entry.config {
                cache.fingerprint = None;
            }
        });
        assert!(config_file_changed(&paths));

        let current = load_data_from_files(&paths, &AppData::default()).unwrap().0;
        let reloaded_config = read_config_file(&paths.config).unwrap();
        let next = reload_data_from_files(&paths, &current, reloaded_config).unwrap();
        assert_eq!(next.providers[0].identity.name, "Renamed");
        assert_eq!(
            next.providers[0].request.endpoints[0].path,
            "/api/user/renamed"
        );
        // 运行期快照仍来自内存，不因配置重读而丢失。
        assert_eq!(next.providers[0].quota.available, 12.5);
        assert!(matches!(
            next.providers[0].runtime.status,
            ProviderStatus::Ok
        ));
    }

    #[test]
    fn save_refuses_to_overwrite_an_external_config_edit() {
        let (_directory, paths) = temp_paths();
        let mut data = fixture();
        save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap();

        let mut config = read_config_file(&paths.config).unwrap();
        config.providers[0].identity.name = "External".into();
        let external = serde_json::to_vec_pretty(&config).unwrap();
        fs::write(&paths.config, &external).unwrap();
        let before = fs::read(&paths.config).unwrap();

        data.providers[0].identity.remark = "local edit".into();
        let error = save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap_err();
        assert!(error.contains("外部修改"));
        assert_eq!(fs::read(&paths.config).unwrap(), before);
    }

    #[test]
    fn replace_mode_allows_explicit_import_to_overwrite() {
        let (_directory, paths) = temp_paths();
        let mut data = fixture();
        save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap();
        let mut config = read_config_file(&paths.config).unwrap();
        config.providers[0].identity.name = "External".into();
        fs::write(&paths.config, serde_json::to_vec_pretty(&config).unwrap()).unwrap();

        data.providers[0].identity.name = "Imported".into();
        save_split(&paths, &data, SaveMode::Replace).unwrap();
        let loaded = load_data_from_files(&paths, &AppData::default()).unwrap().0;
        assert_eq!(loaded.providers[0].identity.name, "Imported");
    }

    #[test]
    fn corrupt_state_only_resets_runtime() {
        let (_directory, paths) = temp_paths();
        let data = fixture();
        save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap();
        fs::write(&paths.state, "{ not json").unwrap();
        let (loaded, warning) = read_state_file(&paths.state);
        assert!(loaded.is_none());
        assert!(warning.is_some());
        let merged = load_data_from_files(&paths, &AppData::default()).unwrap().0;
        assert_eq!(merged.providers[0].identity.name, "Fixture");
        assert_eq!(merged.providers[0].quota.available, 0.0);
        assert!(matches!(
            merged.providers[0].runtime.status,
            ProviderStatus::Warning
        ));
    }

    #[test]
    fn config_requires_stable_unique_ids() {
        let data = fixture();
        let mut config = ConfigData::from_app_data(&data);
        config.providers[0].id = None;
        assert!(config.validate().unwrap_err().contains("缺少 id"));
        config.providers[0].id = Some("same".into());
        config.providers.push(config.providers[0].clone());
        assert!(config.validate().unwrap_err().contains("重复"));
    }

    #[test]
    fn external_config_owns_agent_model_bindings() {
        let (_directory, paths) = temp_paths();
        let data = fixture();
        assert_eq!(data.providers[0].cli.agent_models["codex"], "gpt-5");
        save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap();
        let current = load_data_from_files(&paths, &AppData::default()).unwrap().0;
        // 手写配置删掉整个字段，等同于清空绑定；不能因为编辑器语义
        // （None 表示保留）而让外部配置无法删除绑定。
        let mut value = serde_json::to_value(ConfigData::from_app_data(&data)).unwrap();
        value["providers"][0]["cli"]
            .as_object_mut()
            .unwrap()
            .remove("agentModels");
        fs::write(&paths.config, serde_json::to_vec(&value).unwrap()).unwrap();
        let config = read_config_file(&paths.config).unwrap();
        let next = reload_data_from_files(&paths, &current, config).unwrap();
        assert!(next.providers[0].cli.agent_models.is_empty());
    }

    #[test]
    fn a_stale_reload_does_not_mark_a_newer_external_write_as_loaded() {
        let (_directory, paths) = temp_paths();
        let data = fixture();
        save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap();
        let current = load_data_from_files(&paths, &AppData::default()).unwrap().0;
        // 模拟读取完成、合并尚未返回时，编辑器又写入了下一版配置。
        let stale = read_config_file(&paths.config).unwrap();
        let mut newer = ConfigData::from_app_data(&data);
        newer.providers[0].identity.name = "External".into();
        fs::write(&paths.config, serde_json::to_vec_pretty(&newer).unwrap()).unwrap();
        reload_data_from_files(&paths, &current, stale).unwrap();
        // 读取的内容不是磁盘上的最新版本，必须保留“待重读”状态。
        assert!(config_file_changed(&paths));

        let mut changed = current.clone();
        changed.providers[0].identity.name = "Local".into();
        let error = save_split(&paths, &changed, SaveMode::RespectExternalEdits).unwrap_err();
        assert!(error.contains("外部修改"), "unexpected error: {error}");
        // 冲突保存不能把外部版本标成已加载，否则监听线程不会再重读。
        assert!(config_file_changed(&paths));
        assert_eq!(
            read_config_file(&paths.config).unwrap().providers[0]
                .identity
                .name,
            "External"
        );
    }

    #[test]
    fn a_card_without_site_url_still_loads_instead_of_failing_the_whole_config() {
        let (_directory, paths) = temp_paths();
        let data = fixture();
        save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap();
        let current = load_data_from_files(&paths, &AppData::default()).unwrap().0;
        let mut config = read_config_file(&paths.config).unwrap();
        config.providers[0].identity.base_url.clear();
        fs::write(&paths.config, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        let config = read_config_file(&paths.config).unwrap();
        let next = reload_data_from_files(&paths, &current, config).unwrap();
        assert_eq!(next.providers.len(), 1);
        assert!(next.providers[0].identity.base_url.trim().is_empty());
    }

    #[test]
    fn add_and_remove_providers_follow_the_external_config() {
        let (_directory, paths) = temp_paths();
        let data = fixture();
        save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap();
        let current = load_data_from_files(&paths, &AppData::default()).unwrap().0;

        let mut config = read_config_file(&paths.config).unwrap();
        let mut second = config.providers[0].clone();
        second.id = Some("second".into());
        second.identity.name = "Second".into();
        second.identity.base_url = "https://second.example".into();
        config.providers.push(second);
        fs::write(&paths.config, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        let next = reload_data_from_files(&paths, &current, config).unwrap();
        assert_eq!(next.providers.len(), 2);
        assert_eq!(next.providers[1].identity.id, "second");

        let mut config = read_config_file(&paths.config).unwrap();
        config
            .providers
            .retain(|provider| provider.id.as_deref() == Some("second"));
        let next = reload_data_from_files(&paths, &next, config).unwrap();
        assert_eq!(next.providers.len(), 1);
        assert_eq!(next.providers[0].identity.id, "second");
    }

    #[test]
    fn config_json_with_utf8_bom_is_read_like_plain_json() {
        let (_directory, paths) = temp_paths();
        let data = fixture();
        let config = ConfigData::from_app_data(&data);
        let mut text = serde_json::to_string_pretty(&config).unwrap();
        text.insert(0, '\u{feff}');
        fs::write(&paths.config, text).unwrap();
        let parsed = read_config_file(&paths.config).expect("BOM 不应阻断读取");
        assert_eq!(parsed.providers[0].identity.name, "Fixture");
    }

    #[test]
    fn self_save_does_not_look_like_an_external_config_change() {
        let (_directory, paths) = temp_paths();
        let data = fixture();
        save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap();
        assert!(!config_file_changed(&paths));
        // 只更新运行状态同样不能被误判成外部修改。
        save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap();
        assert!(!config_file_changed(&paths));
    }

    #[test]
    fn invalid_external_config_is_never_overwritten_by_state_saves() {
        let (_directory, paths) = temp_paths();
        let data = fixture();
        save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap();
        fs::write(&paths.config, "{ broken").unwrap();
        let invalid = fs::read(&paths.config).unwrap();

        let mut runtime = data.clone();
        runtime.providers[0].quota.available = 1.0;
        save_state_only(&paths, &runtime).unwrap();
        assert_eq!(fs::read(&paths.config).unwrap(), invalid);
        let error = save_split(&paths, &runtime, SaveMode::RespectExternalEdits).unwrap_err();
        assert!(error.contains("config.json"), "错误应指向具体文件：{error}");
        assert_eq!(fs::read(&paths.config).unwrap(), invalid);
    }

    #[test]
    fn missing_config_is_not_recreated_after_external_deletion() {
        let (_directory, paths) = temp_paths();
        let data = fixture();
        save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap();
        fs::remove_file(&paths.config).unwrap();
        let error = save_split(&paths, &data, SaveMode::RespectExternalEdits).unwrap_err();
        assert!(error.contains("删除"));
        assert!(!paths.config.exists());
    }

    #[test]
    fn config_projection_is_valid_json() {
        let data = fixture();
        let text = String::from_utf8(config_bytes(&data).unwrap()).unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&text).is_ok());
    }
}
