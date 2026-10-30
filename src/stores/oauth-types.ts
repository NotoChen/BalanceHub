import type { Provider } from "./provider-types";

export type OAuthVendor = "codex" | "claude" | "gemini" | "grok";
export type OAuthExportFormat =
  | "balanceHub"
  | "cpa"
  | "ccSwitch"
  | "newApi"
  | "sub2Api"
  | "magpie"
  | "native";
export interface OAuthCredential {
  vendor: OAuthVendor;
  idToken: string;
  accountId: string;
  subject: string;
  email: string;
  organizationId: string;
  organizationName: string;
  projectId: string;
  plan: string;
  scopes: string[];
  clientId: string;
  clientSecret: string;
  source: string;
  refreshOwner: string | null;
  refreshPending: boolean;
  refreshError: string | null;
  updatedAt: number | null;
}
/// One reading of a quota window's used percentage. The providers serve only
/// the current value, so the card draws the trend BalanceHub observed itself.
export interface AccountQuotaSample {
  /// Unix seconds when BalanceHub took the reading.
  at: number;
  usedPercent: number;
}
export interface AccountQuotaWindow {
  id: string;
  label: string;
  model: string | null;
  usedPercent: number | null;
  used: number | null;
  limit: number | null;
  unit: string | null;
  resetAt: number | null;
  windowSeconds: number | null;
  /// Readings carried across refreshes; empty until the account refreshes twice.
  history: AccountQuotaSample[];
}
export interface AccountSubscription {
  plan: string;
  organization: string;
  projectId: string;
  windows: AccountQuotaWindow[];
  credits: number | null;
  creditsUnit: string | null;
  creditsUnlimited: boolean;
  updatedAt: number | null;
  error: string | null;
}
export interface CpaAccountSummary {
  name: string;
  authIndex: string;
  provider: string;
  email: string;
  status: string;
  statusLabel: string;
  disabled: boolean;
  unavailable: boolean;
  available: boolean;
  canDownload: boolean;
  downloadUnavailableReason: string | null;
  models: string[];
  success: number | null;
  failed: number | null;
  priority: number | null;
  weight: number | null;
  note: string;
  excludedModels: string[];
  requestRetry: number | null;
  lastRefresh: number | null;
  nextRetryAfter: number | null;
  quotaExceeded: boolean;
  quotaReason: string;
  quotaNextRecoverAt: number | null;
  backoffLevel: number | null;
  /// The gateway reports that a plugin can read this credential's quota, and on
  /// a v8 gateway which plugin to ask. A v0 gateway leaves the plugin empty and
  /// serves the same reading through its generic quota route.
  supportsQuota: boolean;
  quotaPluginId: string;
  recentRequests: CpaRequestBucket[];
}
export interface CpaRequestBucket {
  time: string;
  success: number;
  failed: number;
}
/// Request counts for one API-key credential the gateway holds in its own
/// configuration. CPA keys these by `baseUrl|key` and groups them by provider;
/// they are not auth files, so the credential list cannot show them.
export interface CpaApiKeyUsage {
  provider: string;
  baseUrl: string;
  key: string;
  success: number;
  failed: number;
  recentRequests: CpaRequestBucket[];
}
/// A summary number a plugin derived from the provider's quota answer, such as
/// a balance or a monthly total.
export interface CpaQuotaMetric {
  key: string;
  label: string;
  value: number;
  unit: string;
  /// `currency` means `value` is money in `currency`; `number` is a plain count.
  format: string;
  currency: string;
}
/// One limit window. `remainingFraction` is what is left (0–1), so the meter
/// draws usage from its complement; a plugin that only knows a reset time
/// leaves it null.
export interface CpaQuotaBucket {
  window: string;
  remainingFraction: number | null;
  resetAt: number | null;
  description: string;
}
export interface CpaQuotaGroup {
  displayName: string;
  buckets: CpaQuotaBucket[];
}
export interface CpaAccountQuota {
  plan: string;
  tier: string;
  metrics: CpaQuotaMetric[];
  groups: CpaQuotaGroup[];
}
export interface CpaErrorLog {
  name: string;
  size: number;
  modifiedAt: number | null;
}
export interface CpaErrorLogContent {
  name: string;
  content: string;
}
export interface CpaPlugin {
  id: string;
  name: string;
  version: string;
  /// The per-plugin switch stored in the gateway configuration.
  enabled: boolean;
  /// What the gateway actually loads: also needs the plugin system on and the
  /// plugin itself to be registered.
  effectiveEnabled: boolean;
  configured: boolean;
  registered: boolean;
  supportsQuota: boolean;
  /// The provider key this plugin registers for its own login, empty when it
  /// does not serve one.
  oauthProvider: string;
}
export interface CpaPluginList {
  enabled: boolean;
  plugins: CpaPlugin[];
}
/// What the gateway removed with a plugin. The file and its saved
/// configuration are deleted together, so credentials it held are gone too.
export interface CpaPluginRemoval {
  fileDeleted: boolean;
  configuredRemoved: boolean;
}
/// One entry a gateway store source offers. `sourceId` names the source the
/// install has to reference; `installedVersion` and `updateAvailable` describe
/// the copy the gateway already holds.
export interface CpaStorePlugin {
  id: string;
  sourceId: string;
  sourceName: string;
  name: string;
  description: string;
  author: string;
  version: string;
  repository: string;
  tags: string[];
  /// The store source needs its own credentials; `authConfigured` says the
  /// gateway's `plugins.store.auth` matches them, so an install can run.
  authRequired: boolean;
  authConfigured: boolean;
  installed: boolean;
  installedVersion: string;
  updateAvailable: boolean;
}
/// A store source the gateway failed to read; the other sources still list.
export interface CpaStoreSourceError {
  sourceId: string;
  sourceName: string;
  message: string;
}
export interface CpaPluginStore {
  enabled: boolean;
  plugins: CpaStorePlugin[];
  sourceErrors: CpaStoreSourceError[];
}
/// What the gateway did on install. `restartRequired` means the running plugin
/// could not be replaced and the new version starts with the next gateway run.
export interface CpaStoreInstall {
  id: string;
  version: string;
  restartRequired: boolean;
}
/// One plugin's saved configuration as JSON text, exactly as the gateway keeps
/// it under `plugins.configs.<id>`.
export interface CpaPluginConfig {
  content: string;
}
/// Gateway-wide scheduling settings. They describe the whole gateway, not one
/// credential, and are read when the panel asks instead of on every refresh.
export interface CpaGatewaySettings {
  routingStrategy: string;
  proxyUrl: string;
  requestRetry: number | null;
  maxRetryCredentials: number | null;
  maxRetryInterval: number | null;
  /// What the gateway does when an account reports its quota exhausted.
  switchProject: boolean | null;
  switchPreviewModel: boolean | null;
  /// Whether the gateway keeps a separate log file for every request.
  requestLog: boolean | null;
  logsMaxTotalSizeMb: number | null;
  errorLogsMaxFiles: number | null;
  /// Whether the gateway records usage that its usage queue reports.
  usageStatisticsEnabled: boolean | null;
  /// Whether debug lines go into the same log file as everything else.
  debug: boolean | null;
  /// Whether a model request must carry its credential's prefix.
  forceModelPrefix: boolean | null;
  /// Whether the AI Studio provider also authenticates WebSocket calls.
  wsAuth: boolean | null;
}
/// Only the values the panel changed are sent; an untouched field keeps the
/// gateway's current value. An empty `proxyUrl` clears the outbound proxy.
export interface CpaGatewaySettingsUpdate {
  routingStrategy?: string;
  proxyUrl?: string;
  requestRetry?: number;
  maxRetryCredentials?: number;
  maxRetryInterval?: number;
  switchProject?: boolean;
  switchPreviewModel?: boolean;
  requestLog?: boolean;
  logsMaxTotalSizeMb?: number;
  errorLogsMaxFiles?: number;
  usageStatisticsEnabled?: boolean;
  debug?: boolean;
  forceModelPrefix?: boolean;
  wsAuth?: boolean;
}
/// Keys downstream clients present to the gateway. They are separate from the
/// management key this card connects with, so a card with a working connection
/// can still have no client key to hand out.
export interface CpaClientKeys {
  keys: string[];
}
/// The saved list plus the key that was added, because a minted key is shown in
/// full once and masked afterwards.
export interface CpaClientKeyAdded {
  list: CpaClientKeys;
  value: string;
}
export interface CpaLogTail {
  lines: string[];
  lineCount: number | null;
  latestAt: number | null;
  /// The position after the last line; a follow read sends it back and gets
  /// only what was written since. Empty when the gateway cannot follow.
  nextCursor: string;
  /// The gateway rotated its logs, so the cursor did not resolve and these
  /// lines are a fresh tail rather than a continuation.
  cursorReset: boolean;
}
/// Whether the gateway writes the log this panel tails. The request log is
/// served only while this is on, so an empty log is a switch, not a failure.
export interface CpaLogSettings {
  enabled: boolean;
}
/// The separate file the gateway keeps for one request while request logging is
/// on. Its lines carry only that request, which the shared tail interleaves with
/// unrelated traffic.
export interface CpaRequestLog {
  requestId: string;
  content: string;
}
/// What clearing the shared log did: the gateway truncates the file it is
/// writing and removes the rotated ones, so `removed` counts files, not lines.
export interface CpaLogClear {
  removed: number;
}
/// The gateway's own configuration document. `etag` names the revision the
/// text was read from, and a save sends it back so a concurrent edit fails
/// instead of being silently replaced.
export interface CpaGatewayConfig {
  content: string;
  etag: string;
  /// `v8` or `v0`; a v0 save carries no revision tag.
  apiVersion: string;
}
export interface CpaCredentialFields {
  priority?: number | null;
  weight?: number | null;
  requestRetry?: number | null;
  note?: string | null;
  excludedModels?: string[] | null;
}
export interface CpaCredentialFile {
  name: string;
  content: string;
  editable: boolean;
  reason: string | null;
}
/// Which of the two gateway-wide model rule maps a write replaces.
export type CpaModelRuleKind = "excludedModels" | "modelAliases";
/// Gateway-wide model rules keyed by OAuth channel, as the JSON object text the
/// gateway returned: excluded models hide an upstream model from clients, while
/// aliases rename one or publish a copy beside it.
export interface CpaModelRules {
  excludedModels: string;
  modelAliases: string;
}
/// One model the gateway can serve for an OAuth channel. The rules editor offers
/// these IDs so the user picks a real model instead of typing one from memory.
export interface CpaModelDefinition {
  id: string;
  displayName: string;
  /// The provider family the gateway sorts this model under.
  modelType: string;
  ownedBy: string;
  contextLength: number | null;
}
/// One authorization service the gateway can sign in to. `plugin` is set when
/// the login is served by a plugin rather than by the gateway itself.
export interface CpaLoginProvider {
  id: string;
  label: string;
  plugin: string | null;
}
/// A login the gateway started and now holds. `apiVersion` says which
/// management generation answered, because a v0 gateway serves the same flow
/// under different routes; `flow` is `device` when the user types a code on the
/// provider's site instead of completing a callback.
export interface CpaLoginStart {
  state: string;
  url: string;
  userCode: string | null;
  expiresIn: number | null;
  flow: string;
  apiVersion: string;
}
/// The pending session as the gateway reports it. A failed session carries the
/// gateway's own reason, since only it knows why the provider refused.
export type CpaLoginStatus =
  | { status: "waiting" }
  | { status: "completed" }
  | { status: "failed"; message: string };
export interface CpaGatewaySnapshot {
  apiVersion: string;
  /// The gateway's own build, as CPA reports it on every management response.
  gatewayVersion: string;
  accounts: CpaAccountSummary[];
  totalRequests: number | null;
  failedRequests: number | null;
  updatedAt: number | null;
}
/// What the gateway considers its current release. `updateAvailable` is null
/// when either version is not a comparable dotted number.
export interface CpaVersionCheck {
  current: string;
  latest: string;
  updateAvailable: boolean | null;
}
export interface OAuthExportOption {
  format: OAuthExportFormat;
  label: string;
  available: boolean;
  reason: string | null;
  instructions: string;
  fileName: string;
}
export interface OAuthAccountDetails {
  provider: Provider;
  refreshable: boolean;
  renewalEnabledHere: boolean;
  renewalElsewhere: boolean;
  exportOptions: OAuthExportOption[];
}
export interface OAuthExportResult {
  provider: Provider;
  fileName: string;
  instructions: string;
  renewalPaused: boolean;
  warning: string | null;
}
export interface OAuthImportFile {
  name: string;
  content: string;
}
export interface OAuthImportEntry {
  id: string;
  vendor: OAuthVendor;
  label: string;
  accountId: string;
  plan: string;
  source: string;
  expiresAt: number | null;
  refreshable: boolean;
  duplicateId: string | null;
}
export interface OAuthImportPreview {
  token: string;
  entries: OAuthImportEntry[];
  errors: string[];
}
export interface OAuthImportRequest {
  token: string;
  entryIds: string[];
  replaceExisting: boolean;
  manageRefresh: boolean;
}
export type OAuthLoginPhase =
  "starting" | "waiting" | "exchanging" | "completed" | "failed" | "canceled";
export interface OAuthLoginTask {
  updatedAt: number;
  id: string;
  revision: number;
  vendor: OAuthVendor;
  phase: OAuthLoginPhase;
  authorizationUrl: string | null;
  userCode: string | null;
  acceptsCallback: boolean;
  expiresAt: number;
  createdAt: number;
  message: string;
  providerId: string | null;
  canCancel: boolean;
  canRetrySave: boolean;
}
