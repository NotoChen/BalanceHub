use serde::{Deserialize, Serialize};

/// Editable scheduling fields of one remote CPA credential. Omitted fields are
/// left untouched remotely; an empty `note` and `excluded_models` clear them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaCredentialFields {
    pub priority: Option<i64>,
    pub weight: Option<i64>,
    /// `None` keeps the remote value; `Some(0)` removes the per-account override.
    pub request_retry: Option<i64>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub excluded_models: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaCredentialFile {
    pub name: String,
    pub content: String,
    pub editable: bool,
    pub reason: Option<String>,
}

/// One request-error log file CPA keeps on the gateway host.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaErrorLog {
    pub name: String,
    pub size: u64,
    pub modified_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaErrorLogContent {
    pub name: String,
    pub content: String,
}

/// One plugin CPA reports on the gateway host. `enabled` is the per-plugin
/// switch stored in the gateway configuration; `effective_enabled` also
/// requires the plugin system to be on and the plugin to be loaded.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub effective_enabled: bool,
    pub configured: bool,
    pub registered: bool,
    pub supports_quota: bool,
    /// The provider key this plugin registers for its own login, empty when it
    /// does not serve one. The panel can then show that the plugin adds a way
    /// to sign in, and the login picker can list it.
    #[serde(default)]
    pub oauth_provider: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaPluginList {
    /// Whether the gateway has the plugin system turned on at all.
    pub enabled: bool,
    pub plugins: Vec<CpaPlugin>,
}

/// One plugin the gateway's store offers. `installed_version` and
/// `update_available` are what the store reports about the copy already on the
/// gateway, so the panel can offer an update instead of a second install.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaStorePlugin {
    pub id: String,
    /// The store source this entry came from; installing names it explicitly
    /// so the gateway does not have to guess when two sources share an id.
    pub source_id: String,
    pub source_name: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub repository: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// The store source needs its own credentials to serve this plugin, and
    /// `auth_configured` reports whether the gateway's `plugins.store.auth`
    /// matches it. An install fails until that authorization is configured, so
    /// the panel says so before the user tries.
    pub auth_required: bool,
    pub auth_configured: bool,
    pub installed: bool,
    pub installed_version: String,
    pub update_available: bool,
}

/// What the gateway removed with a plugin. The plugin file and its saved
/// configuration are deleted together, so any credential it held is gone too.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaPluginRemoval {
    pub file_deleted: bool,
    pub configured_removed: bool,
}

/// A store source the gateway failed to read. One broken source must not hide
/// the plugins the other sources still offer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaStoreSourceError {
    pub source_id: String,
    pub source_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaPluginStore {
    /// Whether the gateway loads plugins at all; an installed plugin does
    /// nothing while this is off.
    pub enabled: bool,
    pub plugins: Vec<CpaStorePlugin>,
    pub source_errors: Vec<CpaStoreSourceError>,
}

/// What the gateway did when it installed a store entry. A plugin whose file is
/// already loaded cannot be replaced in place, so the gateway reports that a
/// restart is needed before the new version runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaStoreInstall {
    pub id: String,
    pub version: String,
    pub restart_required: bool,
}

/// What the gateway says about its own release. `update_available` is absent
/// when either side is not a comparable dotted version.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaVersionCheck {
    pub current: String,
    pub latest: String,
    pub update_available: Option<bool>,
}

/// A plugin's saved configuration, as the JSON object CPA stores under
/// `plugins.configs.<id>`. Kept as text so the panel edits what the gateway
/// holds instead of a lossy subset of it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaPluginConfig {
    pub content: String,
}

/// Which of the two gateway-wide model rule maps a write replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CpaModelRuleKind {
    ExcludedModels,
    ModelAliases,
}

/// Gateway-wide model rules keyed by OAuth channel. Excluded models hide
/// upstream models from clients; aliases rename one or publish a copy next to
/// it. Both are kept as the JSON object text the gateway returned, so the panel
/// edits what CPA holds instead of a lossy subset of it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaModelRules {
    pub excluded_models: String,
    pub model_aliases: String,
}

/// One authorization service the gateway can sign in to. `plugin` is set when
/// the gateway serves the login through a plugin, which only runs while the
/// plugin is enabled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaLoginProvider {
    pub id: String,
    pub label: String,
    pub plugin: Option<String>,
}

/// A login the gateway started and now holds. `api_version` records which
/// management generation answered, because a v0 gateway serves the same flow
/// under different routes; `flow` is `device` when the user types a code on
/// the provider's site instead of completing a callback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaLoginStart {
    pub state: String,
    pub url: String,
    pub user_code: Option<String>,
    pub expires_in: Option<u64>,
    pub flow: String,
    pub api_version: String,
}

/// The pending session as the gateway reports it. A failed session carries the
/// gateway's own reason, since only it knows why the provider refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum CpaLoginStatus {
    Waiting,
    Completed,
    Failed { message: String },
}

/// Tail of the gateway's own log file. CPA only exposes it when it is writing
/// logs to disk, so an unavailable tail is an expected state, not an error.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaLogTail {
    pub lines: Vec<String>,
    /// Lines CPA reports for the requested window; absent on older gateways.
    pub line_count: Option<u64>,
    pub latest_at: Option<i64>,
    /// The position after the last line. A follow read sends it back and gets
    /// only what was written since, instead of re-reading the whole tail.
    #[serde(default)]
    pub next_cursor: String,
    /// CPA rotated or trimmed its logs, so the cursor no longer resolves and
    /// the returned lines are a fresh tail rather than a continuation.
    #[serde(default)]
    pub cursor_reset: bool,
}

/// Whether the gateway writes its log to disk. The request log is served only
/// while this is on, so the panel reads it before offering the log and lets the
/// user turn it on where the empty view would otherwise look like a failure.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaLogSettings {
    pub enabled: bool,
}

/// The separate file the gateway keeps for a single request when request logging
/// is on. Its lines carry everything that request did, which the shared tail
/// interleaves with unrelated traffic.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaRequestLog {
    pub request_id: String,
    pub content: String,
}

/// What clearing the shared log did. The gateway truncates its current file and
/// removes the rotated ones, so the count is the files that are gone rather than
/// lines.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaLogClear {
    pub removed: u64,
}

/// The gateway's own configuration document, verbatim. Values the panel does
/// not model are edited here; the tag names the revision the text came from,
/// because a v8 gateway refuses a save that does not carry it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaGatewayConfig {
    pub content: String,
    /// Opaque revision tag; empty on a v0 gateway, which has no such guard.
    #[serde(default)]
    pub etag: String,
    /// Which management generation served the document, echoed back on save.
    #[serde(default)]
    pub api_version: String,
}

/// One model the gateway itself can serve for an OAuth channel. The panel uses
/// this list to offer the channel's real IDs instead of asking the user to type
/// them into an exclusion rule.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpaModelDefinition {
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    /// The provider family the gateway sorts this model under.
    #[serde(rename = "type", default)]
    pub model_type: String,
    #[serde(default)]
    pub owned_by: String,
    /// The model's context window in tokens, when the gateway declares one.
    #[serde(default)]
    pub context_length: Option<i64>,
}
