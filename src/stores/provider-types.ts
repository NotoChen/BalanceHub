import type { OAuthCredential, AccountQuotaSample, AccountSubscription, CpaGatewaySnapshot } from "./oauth-types";
import type { ModelPrice } from "../api/model-catalog";
import type {
  SessionMessageMetadata,
  SessionFileChange,
  SessionReadPage,
  SessionTurnSummary,
  SessionFindState,
} from "./session-reader-types";import type { AgentCliKind } from "../agent-cli/visuals.ts";
import type { AgentSessionLaunchIdentity } from "./agent-session-types";

export type { AgentCliKind } from "../agent-cli/visuals.ts";
export type * from "./agent-catalog-types";
export type * from "./agent-lifecycle-types";
export type * from "./agent-session-types";

export type AuthMode = "apiKey" | "accessToken" | "session" | "password";
export type AuthSource = "manual" | "password" | "oauth";
export type ProviderProtocol = "newApi" | "sub2Api" | "api" | "oauth" | "cliProxyApi";

export interface ProviderAuthModeDescriptor {
  mode: AuthMode;
  label: string;
  description: string;
  note: string;
  requiredFields: string[];
  requiredAnyFields: string[];
  optionalFields: string[];
  fields: ProviderAuthFieldDescriptor[];
}

export interface ProviderAuthFieldDescriptor {
  field: string;
  label: string;
  placeholder: string;
  secret: boolean;
  wide: boolean;
  readonly: boolean;
  showWhenEmpty: boolean;
}

export interface ProviderProtocolCapabilitiesDescriptor {
  accessToken: boolean;
  apiKeyManagement: boolean;
  usage: boolean;
  account: boolean;
  /** 允许在应用内修改用户名和显示名。 */
  accountProfile: boolean;
  /** 提供兑换码入口。 */
  accountRedemption: boolean;
  checkIn: boolean;
  announcements: boolean;
  /// 可填写的备用地址数量上限；0 表示该协议的请求不经过中转站传输层，
  /// 备用地址不会生效。
  maxBackupUrls: number;
  /// 额外请求头会附加到该协议的每个请求。
  requestHeaders: boolean;
  /// 该协议读取卡片声明的 API Key 发送方式。
  apiKeyScheme: boolean;
  /// 该协议读取卡片声明的余额接口。
  quotaSpec: boolean;
  /// 该协议允许声明的订阅窗口数量上限；0 表示不读取余额接口。
  maxQuotaWindows: number;
  /// 该协议读取卡片声明的模型列表地址。
  modelsUrl: boolean;
  /// 该协议允许按接口改写的数量上限；0 表示没有请求目录。
  maxEndpointOverrides: number;
  /// 请求日志支持的筛选维度；界面只渲染这里列出的控件。
  requestLogFilters: ProviderRequestLogFilter[];
}

export type ProviderRequestLogFilter =
  | "model"
  | "tokenName"
  | "group"
  | "logType"
  | "timeRange"
  | "requestId";

export interface ProviderEndpointHeaderDescriptor {
  name: string;
  value: string;
}

/// 协议会对站点发出的一个请求；编辑器把它作为可见的默认行为展示。
export interface ProviderEndpointDescriptor {
  id: string;
  group: string;
  groupLabel: string;
  label: string;
  description: string;
  /// null 表示不是 HTTP 请求，而是可复制的外部链接模板。
  method: string | null;
  path: string;
  query: string;
  altPaths: string[];
  placeholders: string[];
  headers: ProviderEndpointHeaderDescriptor[];
  auth: "none" | "account" | "apiKey";
  authLabel: string;
  jsonBody: boolean;
}

export interface ProviderProtocolOperationMethodsDescriptor {
  checkIn: string | null;
  apiKeys: string | null;
  invitation: string | null;
  models: string;
  announcements: string | null;
  pricing: string | null;
  usage: string | null;
  requestLogs: string | null;
  subscription: string | null;
  groups: string | null;
}

export interface ProviderCredentialAssistantDescriptor {
  enabled: boolean;
  accessTokenFlow: "none" | "credentialCompletion" | "sessionGeneration";
  accessTokenSkipFields: string[];
  apiKeyRequiredFields: string[];
  apiKeyRequiredAnyFields: string[];
}

export interface ProviderProtocolDescriptor {
  formCreation: boolean;
  kind: ProviderProtocol;
  label: string;
  description: string;
  defaultAuthMode: AuthMode;
  browserLoginSupported: boolean;
  authModes: ProviderAuthModeDescriptor[];
  capabilities: ProviderProtocolCapabilitiesDescriptor;
  operationMethods: ProviderProtocolOperationMethodsDescriptor;
  /// 该协议会发出的请求默认值，按目录顺序排列。
  endpoints: ProviderEndpointDescriptor[];
  credentialAssistant: ProviderCredentialAssistantDescriptor;
}
export type ProviderQuotaScope = "account" | "token";
export type ProviderStatus = "ok" | "warning" | "error" | "syncing";
export type ProxyMode = "system" | "noProxy" | "custom";
export type ProviderProxyMode = "inherit" | "system" | "noProxy" | "custom";
export type ProviderNotificationMode = "inherit" | "custom" | "disabled";
export type ThemeMode = "system" | "light" | "dark";
export type WindowCloseAction = "hideToTray" | "quit";
export type TrayClickAction = "showWindow" | "toggleWindow";
export type LogLevel = "error" | "warn" | "info" | "debug";
export type LivenessIntervalMode = "fixed" | "random";
export type LivenessPromptMode = "fixed" | "random" | "roundRobin";
export type TemporaryCliInstanceStatus = "starting" | "running" | "exited";
export type TemporaryCliSessionMode = "new" | "history";
export type TemporaryCliTerminalKind =
  | "terminal"
  | "iTerm2"
  | "warp"
  | "wezTerm"
  | "ghostty"
  | "kitty"
  | "alacritty"
  | "kaku"
  | "windowsTerminal"
  | "commandPrompt"
  | "powerShell";
export type NotificationChannelKind =
  "system" | "dingtalk" | "wecom" | "feishu" | "slack" | "generic";

export interface Provider {
  revision: number;
  displayLabel: string;
  protocolLabel: string;
  protocolDescription: string;
  authModeLabel: string;
  authModeDescription: string;
  identity: ProviderIdentity;
  auth: ProviderAuth;
  quota: ProviderQuota;
  capabilities: ProviderCapabilities;
  cli: ProviderCli;
  automation: ProviderAutomation;
  liveness: ProviderLiveness;
  proxy: ProviderProxy;
  notification: ProviderNotification;
  runtime: ProviderRuntime;
  /// 卡片对上游的请求约定：额外请求头与 API Key 发送方式。
  request: ProviderRequestConfig;
  /// 通用 API Key 卡片的余额读取声明；未配置时为空对象。
  quotaSpec: ProviderQuotaSpec;
  /// 模型展示与元数据覆盖；原始模型列表仍在 capabilities.availableModels。
  modelConfig: ProviderModelConfig;
  /// 应用展示规则（include / exclude / hidden）之后的模型列表。
  visibleModels: string[];
  /// 主地址失败、请求改走备用地址时由 Rust 给出；主地址正常时为 null。
  activeEndpoint?: ProviderActiveEndpoint | null;
  actions: ProviderActions;
}

export interface ProviderActiveEndpoint {
  /// 备用地址原文，保留用户填写的写法。
  backupUrl: string;
  /// 备用地址在列表中的位置，从 1 开始。
  backupIndex: number;
  switchedAt: number;
}

export interface ProviderActions {
  temporaryCli: boolean;
  defaultCliConfig: boolean;
  oauthAccount: boolean;
  cpaGateway: boolean;
  appImports: ProviderAppImportOption[];
  accountManagement: boolean;
  /** 编辑站点账号的用户名和显示名。 */
  editProfile: boolean;
  /** 使用兑换码补充余额。 */
  redeemCode: boolean;
  checkIn: boolean;
  checkedInToday: boolean;
  apiKeyManagement: boolean;
  invitation: boolean;
  /** 站点级用量趋势；能力探测确认不支持时为 false。 */
  usage: boolean;
  /** 站点请求日志；能力探测确认不支持时为 false。 */
  requestLogs: boolean;
  /** 站点公告；能力探测确认不支持时为 false。 */
  announcements: boolean;
  refreshModelsOnly: boolean;
  models: {
    canSync: boolean;
    scope: ProviderModelScope;
    unavailableReason: string | null;
  };
}

export type CcSwitchAgent = "codex" | "claude" | "gemini" | "grokbuild" | "opencode" | "openclaw" | "hermes";

export type ProviderAppImportTarget =
  | { app: "magpie" }
  | { app: "ccSwitch"; agent: CcSwitchAgent };

export interface ProviderAppImportOption {
  target: ProviderAppImportTarget;
  label: string;
  unavailableReason: string | null;
  note: string | null;
}

export interface ProviderAppImportResult {
  appLabel: string;
  targetLabel: string;
  launchError: string | null;
  importUrl: string | null;
}

export interface ProviderIdentity {
  id: string;
  name: string;
  baseUrl: string;
  protocol: ProviderProtocol;
  remark: string;
  displayName: string;
  username: string;
  userId: string;
  siteLogo: string;
  backupUrls: string[];
}

export interface ProviderIdentityInput {
  name: string;
  baseUrl: string;
  protocol: ProviderProtocol;
  remark: string;
  userId: string;
  backupUrls: string[];
}

export interface ProviderCli {
  preferredModel: string;
}

export interface ProviderCliInput {
  preferredModel: string;
}

export interface NewApiSession {
  refreshCookie: string;
  sessionId: string;
  accessToken: string;
  accessExpiresAt: number | null;
}

export interface ProviderAuth {
  oauth: OAuthCredential | null;
  mode: AuthMode;
  source?: AuthSource;
  apiKey: string;
  apiKeyTokenId: string;
  apiKeyOptions: ProviderApiKeyOption[];
  accessToken: string;
  sessionCookie: string;
  apiUser: string;
  loginUsername: string;
  loginPassword: string;
  refreshToken: string;
  accessTokenExpiresAt?: number | null;
  newApiSession?: NewApiSession | null;
  browserBinding?: BrowserLoginBinding | null;
  credentialRevision?: number;
  sessionUpdatedAt?: number | null;
}

export type LoginPlatform = "linuxDo" | "github" | "other" | "unknown";
export interface BrowserLoginBinding {
  accountId: string | null;
  platform: LoginPlatform;
  mechanism: "oauth" | "password" | "unknown";
  importedAt: number;
}
export interface LoginAccount {
  id: string;
  name: string;
  platform: LoginPlatform;
  identity: string | null;
  createdAt: number;
  lastOpenedAt: number | null;
  lastUsedAt: number | null;
  identityObservedAt: number | null;
  generation: number;
}
export interface LoginAccountSummary extends LoginAccount {
  platformLabel: string;
  profilePresent: boolean;
  cookieCount: number;
  busy: boolean;
  canLogin: boolean;
  sessionLabel: string;
  canOpen: boolean;
  sessionProblem: string | null;
  linkedProviders: { id: string; name: string }[];
}
export interface LoginCookieSummary {
  id: string;
  name: string;
  domain: string;
  path: string;
  expires: number;
  httpOnly: boolean;
  secure: boolean;
}
export type CredentialKind = "dashboardJwt" | "refreshCookie" | "sessionCookie" | "accessToken" | "refreshToken" | "apiKey" | "password";
export interface CredentialSummary {
  kind: CredentialKind;
  label: string;
  source: string;
  expiresAt: number | null;
  status: string;
  clearLabel: string | null;
}
export interface ProviderCredentialDetails {
  providerId: string;
  providerName: string;
  credentialRevision: number;
  binding: BrowserLoginBinding | null;
  accountName: string | null;
  sessionId: string | null;
  updatedAt: number | null;
  verifiedAt: string | null;
  error: string | null;
  authenticationLabel: string;
  validationLabel: string;
  validationScope: string;
  entries: CredentialSummary[];
  canLogin: boolean;
  canValidate: boolean;
}

export interface ProviderQuota {
  available: number;
  used: number;
  known?: boolean;
  totalKnown?: boolean;
  scope?: ProviderQuotaScope;
  unlimited?: boolean;
  perUnit: number;
  displayType: string;
  currencySymbol: string;
  currencyExchangeRate: number;
  /** 账号当前使用的站点分组；NewAPI 价格要乘分组倍率。 */
  group?: string;
  /** 订阅制中转站的周期额度窗口；钱包在这里只是溢出账户。 */
  windows?: ProviderQuotaWindow[];
}

export interface ProviderQuotaWindow {
  label: string;
  used: number;
  /** 0 表示不限量。 */
  limit: number;
  /** unix 秒。 */
  resetsAt?: number | null;
  /** 本应用刷新时观察到的读数；旧数据或首次刷新时为空。 */
  history?: AccountQuotaSample[];
}

export interface ProviderCapabilities {
  subscription: AccountSubscription | null;
  gateway: CpaGatewaySnapshot | null;
  checkInKnown: boolean;
  checkInSupported: boolean;
  checkInAuthModes: AuthMode[];
  apiKeyManagementKnown: boolean;
  apiKeyManagementSupported: boolean;
  invitationKnown: boolean;
  invitationSupported: boolean;
  inviteLink: string;
  probedAt: string | null;
  errorMessage?: string | null;
  availableModels: string[];
  availableModelsState: ProviderModelListState;
  /** 定价、用量、日志、公告、订阅、分组、模型的探测结果。 */
  optional: ProviderOptionalCapabilityState[];
  /** 站点价目快照；推荐、用量对账和模型详情共用。 */
  pricing: ProviderPricingSnapshot;
}

export type ProviderOptionalCapability =
  | "pricing"
  | "usage"
  | "requestLogs"
  | "announcements"
  | "subscription"
  | "groups"
  | "models";

export interface ProviderOptionalCapabilityState {
  capability: ProviderOptionalCapability;
  /** 是否得到明确结论；网络错误时为 false。 */
  known: boolean;
  supported: boolean;
  reason: string | null;
  error: string | null;
}

export type ProviderPricingSource = "none" | "newApi" | "sub2Api";

export interface ProviderModelPrice {
  /** USD / 百万 token。 */
  input: number | null;
  output: number | null;
  cacheRead: number | null;
  cacheWrite: number | null;
  /** 按次计费单价（USD / 次）；有值时不再按 token 估算。 */
  perRequest: number | null;
  group: string;
}

export interface ProviderPricingSnapshot {
  models: Record<string, ProviderModelPrice>;
  groups: Record<string, number>;
  group: string;
  source: ProviderPricingSource;
  updatedAt: string | null;
  error: string | null;
}

export type ProviderModelScope = "account" | "apiKey";

export interface ProviderModelListState {
  scope: ProviderModelScope | null;
  updatedAt: string | null;
  error: string | null;
  fallbackReason: string | null;
}

export type ProviderCheckInMethod = "auto" | "standard" | "sessionSignIn" | "freshLogin";
export type ProviderTurnstileMode = "auto" | "always";

export interface ProviderCheckInPolicyPreview {
  method: Exclude<ProviderCheckInMethod, "auto">;
  methodLabel: string;
  configurable: boolean;
  supported: boolean;
  message: string;
}

export interface ProviderAutomation extends ProviderAutomationInput {
  lastSyncedAt: string | null;
  lastCheckedInAt: string | null;
  lastCheckInUser: string;
  checkInRecords: ProviderCheckInRecord[];
}

export interface ProviderAutomationInput {
  refreshInterval: number;
  checkInTime: string;
  checkInMethod: ProviderCheckInMethod;
  autoShield: boolean;
  turnstileMode: ProviderTurnstileMode;
}

export interface ProviderLiveness {
  useGlobal: boolean;
  enabled: boolean;
  agentBaseUrls: Partial<Record<AgentCliKind, string>>;
  cliKind?: AgentCliKind | null;
  intervalMode: LivenessIntervalMode;
  interval: number;
  randomMinInterval: number;
  randomMaxInterval: number;
  timeout: number;
  model: string;
  promptMode: LivenessPromptMode;
  fixedPrompt: string;
  promptCursor: number;
  nextAt: string | null;
  records: LivenessRecord[];
  runCount: number;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalTokens: number;
  totalCostUsd: number;
}

export interface ProviderLivenessInput {
  useGlobal: boolean;
  enabled: boolean;
  agentBaseUrls: Partial<Record<AgentCliKind, string>>;
  cliKind?: AgentCliKind | null;
  intervalMode: LivenessIntervalMode;
  interval: number;
  randomMinInterval: number;
  randomMaxInterval: number;
  timeout: number;
  model: string;
  promptMode: LivenessPromptMode;
  fixedPrompt: string;
}

export interface ProviderProxy {
  mode: ProviderProxyMode;
  url: string;
}

export interface ProviderNotification {
  mode: ProviderNotificationMode;
  channelIds: string[];
}

export type ProviderAuthScheme = "bearer" | "xApiKey" | "none";

export interface ProviderHeader {
  name: string;
  value: string;
}

export interface ProviderRequestConfig {
  headers: ProviderHeader[];
  authScheme: ProviderAuthScheme;
  /// 按接口改写的路径、查询参数和请求头；未声明的接口沿用协议默认值。
  endpoints: ProviderEndpointOverride[];
}

export interface ProviderEndpointQuery {
  name: string;
  value: string;
}

export interface ProviderEndpointOverride {
  /// 协议请求目录里的接口 id。
  operation: string;
  /// 自定义路径；空表示沿用默认路径，可继续使用 {id} 这类占位符。
  path: string;
  /// 查询参数增量；值留空表示删除同名默认参数。
  query: ProviderEndpointQuery[];
  /// 只附加到该接口的请求头。
  headers: ProviderHeader[];
}

export type ProviderQuotaMethod = "get" | "post";

export interface ProviderQuotaWindowSpec {
  label: string;
  used: string;
  limit: string;
  resetsAt: string;
}

/// 通用 API Key 中转站的余额读取声明。表达式按 perUnit 换算后写入 ProviderQuota。
export interface ProviderQuotaSpec {
  url: string;
  method: ProviderQuotaMethod;
  token: string;
  headers: ProviderHeader[];
  balance: string;
  total: string;
  windows: ProviderQuotaWindowSpec[];
  /// 表达式数值的换算单位；0 表示沿用卡片默认值。
  perUnit: number;
  currencySymbol: string;
}

export interface ProviderModelOverride {
  displayName: string;
  wireName: string;
  alias: string;
  contextWindow: number | null;
  outputLimit: number | null;
  /** USD per million tokens；null 表示沿用目录或留空。 */
  price: ModelPrice;
  reasoningLevels: string[];
  sendsReasoning: boolean | null;
  hidden: boolean;
}

export interface ProviderModelConfig {
  modelsUrl: string;
  include: string[];
  exclude: string[];
  catalog: string;
  overrides: Record<string, ProviderModelOverride>;
}

export interface ProviderRuntime {
  enabled: boolean;
  status: ProviderStatus;
  errorMessage?: string | null;
  consecutiveFailures?: number;
  /// unix 秒；冷却结束前调度器不再刷新该卡片。
  cooldownUntil?: number | null;
  cooldownReason?: string | null;
}

export interface ProviderInput {
  id?: string;
  identity: ProviderIdentityInput;
  auth: ProviderAuth;
  cli: ProviderCliInput;
  automation: ProviderAutomationInput;
  liveness: ProviderLivenessInput;
  proxy: ProviderProxy;
  notification: ProviderNotification;
  request: ProviderRequestConfig;
  quotaSpec: ProviderQuotaSpec;
  modelConfig: ProviderModelConfig;
  runtime: Pick<ProviderRuntime, "enabled">;
}

export type ProviderSaveConflictKind =
  "sameAccount" | "sameApiKey" | "sameUrlDifferentApiKey";

export interface ProviderSaveOptions {
  overwriteProviderId?: string;
  mergeApiKeyIntoProviderId?: string;
  createSeparateFromProviderId?: string;
}

export interface ProviderSaveConflict {
  kind: ProviderSaveConflictKind;
  existingProviderId: string;
  existingProviderName: string;
}

export interface ProviderSaveResult {
  saved: boolean;
  provider: Provider | null;
  conflict: ProviderSaveConflict | null;
}

export interface ProviderRemovalResult {
  id: string;
  revision: number;
}

export interface LivenessRecord {
  checkedAt: string;
  source?: "manual" | "automatic" | string;
  cliKind?: AgentCliKind | string;
  ok: boolean;
  latencyMs: number;
  model: string;
  baseUrl: string;
  prompt: string;
  responsePreview: string;
  responseRaw?: string;
  inputTokens?: number | null;
  cachedInputTokens?: number | null;
  outputTokens?: number | null;
  reasoningOutputTokens?: number | null;
  totalTokens?: number | null;
  totalCostUsd?: number | null;
  message: string;
  commandPreview: string;
}

export interface AgentCliDescriptor {
  kind: AgentCliKind;
  label: string;
  executable: string;
  sessionNameHint: string;
  capabilities: AgentCliCapabilities;
}

export interface CliToolProbeResult extends AgentCliDescriptor {
  available: boolean;
  path: string;
  version: string;
  message: string;
}

export interface AgentCliCapabilities {
  temporaryLaunch: boolean;
  modelSelection: boolean;
  sessionHistory: boolean;
  sessionSearch: boolean;
  sessionDetail: boolean;
  sessionResume: boolean;
  sessionName: boolean;
  liveness: boolean;
  defaultConfig: boolean;
  assetInventory: boolean;
  hookInventory: boolean;
  runtimeObservation: boolean;
}

export interface TemporaryTerminalProbeResult {
  available: boolean;
  kind: TemporaryCliTerminalKind;
  name: string;
  version: string;
  message: string;
}

export interface CliEnvironmentProbeResult {
  tools: CliToolProbeResult[];
}

/** Rust-owned Agent environment inventory and asset control contracts. */
export type AgentEnvironmentKind = "native";
export type AgentHostPlatform = "macos" | "linux" | "windows";
export type AgentHostArchitecture = "aarch64" | "x86_64" | "unknown";
export type AgentCliDistribution =
  "npm" | "homebrew" | "vendorNative" | "winGet" | "apt" | "dnf" | "apk" | "unknown";
export type AgentEnvironmentCapability = "readOnlyInventory" | "boundedPreview";
export type AgentAssetCategory =
  "skill" | "plugin" | "extension" | "mcp" | "hook" | "statusUi";
export type AgentAssetScope =
  "user" | "workspace" | "local" | "system" | "managed";
export type AgentAssetState =
  "enabled" | "disabled" | "notInstalled" | "shadowed" | "blocked" | "invalid" | "unknown";
export type AgentAssetMutation =
  "readOnly" | "nativeToggle" | "managedMutation" | "externalCommand";
export type AgentAssetOpenTarget = "asset" | "reveal";
export type AgentTrustState = "trusted" | "untrusted" | "required" | "unknown";
export type AgentInstallationChannel =
  "stable" | "preview" | "nightly" | "unknown";
export type AgentVersionSource = "npmRegistry" | "localExecutable" | "unknown";
export type AgentDiscoverySource = "configured" | "automatic";
export type AgentInstallationAvailability = "available" | "unavailable";
export type AgentAssetLimitKind =
  | "candidatePathsPerAgent"
  | "installationsPerAgent"
  | "sourcesPerContext"
  | "firstLevelEntries"
  | "frontmatterLines"
  | "frontmatterBytes"
  | "bytesPerSource"
  | "bytesPerRefresh"
  | "refreshBudget"
  | "cliOutput"
  | "cliConcurrency"
  | "watchers"
  | "diagnostics";
export type AgentAssetDocumentFormat = "json" | "toml" | "yaml" | "manifest" | "unknown";
export type AgentAssetSourceKind = "file" | "directory";
export type AgentAssetIoErrorKind =
  "notFound" | "permissionDenied" | "invalidData" | "other";
export type AgentExecutableProbeErrorKind =
  | "notFound"
  | "permissionDenied"
  | "timedOut"
  | "invalidVersion"
  | "changedDuringProbe"
  | "failed";
export type AgentAssetRelationKind =
  "providedBy" | "actionOwner" | "explicitImpact";
export type AgentAssetPresence =
  "present" | "missing" | "invalid" | "blocked" | "unknown";
export type AgentAssetDeclaredState =
  "enabled" | "disabled" | "pending" | "rejected" | "unknown";
export type AgentAssetResolutionRelation =
  | "independent"
  | "replaceWinner"
  | "replaced"
  | "merged"
  | "additive"
  | "unknown";
export type AgentAssetResolutionTerminal = "policyBlocked" | "unknown";
export type AgentAssetInstallState =
  | "installed"
  | "notInstalled"
  | "unknown";
export type AgentAssetDeclarationRole =
  | "definition"
  | "stateOverlay"
  | "policyOverlay";
export type AgentAssetSuppressionReason =
  | "untrustedWorkspace"
  | "compatibilitySourceDisabled"
  | "unsupportedContext"
  | "duplicatePhysicalSource"
  | "parentNotSelected"
  | "unknown";
export type AgentAssetResolutionParticipation =
  | { kind: "participates" }
  | { kind: "suppressed"; reason: AgentAssetSuppressionReason };
export type AgentAssetDiscoveryIncompleteReason =
  | "unsupportedVersion"
  | "installationUnverified"
  | "sourceUnavailable"
  | "runtimeStateUnobserved"
  | "unsupportedEntryPoint"
  | "nativeEquivalenceUnobserved";

export type AgentAssetDiagnostic =
  | {
      kind: "truncated";
      limit: AgentAssetLimitKind;
      accepted: number;
      observedAtLeast: number;
    }
  | {
      kind: "malformed";
      format: AgentAssetDocumentFormat;
      location: string | null;
    }
  | {
      kind: "duplicateNativeId";
      category: AgentAssetCategory;
      nativeId: string;
    }
  | { kind: "unknownField"; fieldPath: string }
  | { kind: "symlinkRejected"; sourceId: string }
  | { kind: "budgetExceeded"; elapsedMs: number; budgetMs: number }
  | { kind: "readFailed"; sourceId: string; errorKind: AgentAssetIoErrorKind }
  | { kind: "invalidNativeId"; category: AgentAssetCategory }
  | {
      kind: "unresolvedRelationship";
      relation: AgentAssetRelationKind;
      nativeId: string;
    }
  | { kind: "invalidProjection"; projectionKey: string }
  | {
      kind: "invalidResolution";
      projectionKey: string;
      resolution: AgentAssetResolutionRelation;
    }
  | {
      kind: "installationProbeFailed";
      candidateSource: AgentDiscoverySource;
      errorKind: AgentExecutableProbeErrorKind;
    }
  | { kind: "sourceOutsideAllowedRoot"; sourceId: string }
  | {
      kind: "sourceTypeMismatch";
      sourceId: string;
      expected: AgentAssetSourceKind;
      actual: AgentAssetSourceKind;
    }
  | { kind: "invalidCompatibleInstallation"; installationId: string }
  | { kind: "declarationSuppressed"; reason: AgentAssetSuppressionReason }
  | {
      kind: "discoveryIncomplete";
      agentKind: AgentCliKind;
      category: AgentAssetCategory;
      reason: AgentAssetDiscoveryIncompleteReason;
    }
  | { kind: "policyBlocked" };
export type AgentAssetPolicyReference =
  | { kind: "declaration"; declarationId: string }
  | { kind: "source"; sourceId: string };
export interface AgentAssetRevision {
  identity: string;
  observedAt: string;
  sizeBytes: number | null;
  isMissing: boolean;
  isDirectory: boolean;
  isSymlink: boolean;
}
export interface AgentExecutableIdentity {
  owner: string;
  canonicalPath: string;
  installationSource: AgentDiscoverySource;
}
export interface AgentConfigurationContext {
  id: string;
  environmentId: string;
  agentKind: AgentCliKind;
  configRoot: string;
  profile: string;
  workspaceId: string | null;
  trustContext: AgentTrustState;
  parserVersion: number;
  schemaFacts: Record<string, string>;
  compatibleInstallationIds: string[];
}
export interface AgentAssetNativeRef {
  category: AgentAssetCategory;
  nativeId: string;
  qualifier: string | null;
}
export type AgentMcpTransport =
  "stdio" | "http" | "sse" | "webSocket" | "unknown";
export type AgentMcpApprovalState =
  "approved" | "rejected" | "pending" | "notRequired" | "unknown";
export type AgentAssetEffectiveAvailability =
  | "available"
  | "disabled"
  | "approvalRequired"
  | "policyBlocked"
  | "trustRequired"
  | "invalid"
  | "unknown";
export type AgentSkillInvocationPolicy =
  "modelInvocable" | "manualOnly" | "disabled" | "unknown";
export type AgentStatusUiMode = "builtIn" | "command" | "disabled" | "unknown";
export type AgentAssetDetails =
  | {
      kind: "skill";
      enabled: AgentAssetDeclaredState;
      invocationPolicy: AgentSkillInvocationPolicy;
    }
  | {
      kind: "mcp";
      transport: AgentMcpTransport;
      declaredState: AgentAssetDeclaredState;
      approvalState: AgentMcpApprovalState;
      effectiveAvailability: AgentAssetEffectiveAvailability;
    }
  | {
      kind: "plugin";
      installState: AgentAssetInstallState;
      enabled: AgentAssetDeclaredState;
      trusted: AgentTrustState;
    }
  | {
      kind: "extension";
      installState: AgentAssetInstallState;
      enabled: AgentAssetDeclaredState;
      trusted: AgentTrustState;
    }
  | { kind: "hook"; managed: boolean; enabled: AgentAssetDeclaredState; ruleCount: number | null }
  | { kind: "statusUi"; mode: AgentStatusUiMode; commandPresent: boolean };

export interface AgentEnvironmentDescriptor {
  id: string;
  kind: AgentEnvironmentKind;
  hostPlatform: AgentHostPlatform;
  hostArchitecture: AgentHostArchitecture;
  guestPlatform: string | null;
  displayName: string;
  capabilities: AgentEnvironmentCapability[];
}

export interface AgentAssetCapability {
  category: AgentAssetCategory;
  discovery: AgentAssetScope[];
  mutation: AgentAssetMutation;
  requiresRestart: boolean;
  requiresTrust: boolean;
}

export type AgentAssetProvision = "agentBuiltIn" | "pluginProvided" | "independent" | "unknown";
export type AgentAssetProviderOrigin = "agentVendor" | "thirdParty" | "userDeclared" | "unknown";
export type AgentAssetInstallationOrigin =
  | "bundled" | "nativePackage" | "localFiles" | "sharedFiles" | "linked" | "configEntry" | "unknown";
export interface AgentAssetProvenance {
  sourceId: string;
  declarationId: string;
  scope: AgentAssetScope;
  provision: AgentAssetProvision;
  installation: AgentAssetInstallationOrigin;
  provider: AgentAssetProviderOrigin;
}
export interface AgentAssetProvenanceSummary {
  provisions: AgentAssetProvision[];
  installations: AgentAssetInstallationOrigin[];
  providers: AgentAssetProviderOrigin[];
}

export interface AgentAssetSource {
  id: string;
  contextId: string;
  label: string;
  scope: AgentAssetScope;
  origin: AgentAssetInstallationOrigin;
  environmentId: string;
  workspaceId: string | null;
  path: string;
  precedence: number;
  writable: boolean;
  sensitive: boolean;
  sourceKind: AgentAssetSourceKind;
  categories: AgentAssetCategory[];
  revision: AgentAssetRevision;
  diagnostics: AgentAssetDiagnostic[];
  access: AgentAssetAccess;
  actions: AgentAssetAction[];
}

export interface AgentCapabilities {
  agentKind: AgentCliKind;
  assets: AgentAssetCapability[];
}

export interface AgentInstallation {
  id: string;
  environmentId: string;
  agentKind: AgentCliKind;
  label: string;
  availability: AgentInstallationAvailability;
  executablePath: string | null;
  executableIdentity: AgentExecutableIdentity | null;
  executableRevision: string | null;
  installedVersion: string | null;
  discoverySource: AgentDiscoverySource;
  distribution: AgentCliDistribution;
  channel: AgentInstallationChannel;
  installedVersionSource: AgentVersionSource;
  diagnostics: AgentAssetDiagnostic[];
}

export interface AgentAssetRecord {
  stableId: string;
  agentKind: AgentCliKind;
  category: AgentAssetCategory;
  nativeId: string;
  label: string;
  sourceIds: string[];
  inspectionSourceId: string;
  scope: AgentAssetScope;
  provenance: AgentAssetProvenance[];
  environmentId: string;
  workspaceId: string | null;
  path: string | null;
  precedence: number;
  writable: boolean;
  declaredState: AgentAssetState;
  effectiveState: AgentAssetState;
  trustState: AgentTrustState;
  diagnostics: AgentAssetDiagnostic[];
  revision: AgentAssetRevision;
  sensitive: boolean;
  isDirectory: boolean;
  contextId: string;
  representedDeclarationIds: string[];
  resolution: AgentAssetResolution;
  relationships: AgentAssetRelationships;
  actions: AgentAssetAction[];
  access: AgentAssetAccess;
  compatibleInstallationIds: string[];
  selectedActionInstallationId: string | null;
  details: AgentAssetDetails;
}

export interface AgentAssetResolution {
  relation: AgentAssetResolutionRelation;
  qualifiedCollision: boolean;
  terminal: AgentAssetResolutionTerminal | null;
  contributorIds: string[];
  winnerId: string | null;
  controlSource: AgentAssetPolicyReference | null;
  diagnostics: AgentAssetDiagnostic[];
}
export interface AgentAssetRelationships {
  providedBy: string | null;
  actionOwner: string | null;
  affectedAssetIds: string[];
}
export type AgentAssetActionKind =
  "inspect" | "enable" | "disable" | "remove" | "preview" | "open" | "reveal";
export type AgentAssetActionUnavailableReason =
  | "mutationDisabled"
  | "noOfficialMechanism"
  | "unsupportedPlatform"
  | "unsupportedScope"
  | "unsupportedSchema"
  | "noCompatibleInstallation"
  | "installationUnavailable"
  | "assetNotInstalled"
  | "assetInstallationUnknown"
  | "ambiguousMechanism"
  | "nativeInteractiveOnly"
  | "invocationPolicyOnly"
  | "noReversibleMechanism"
  | "managedByAssetCatalog"
  | "trustRequired"
  | "scopeAmbiguous"
  | "childOwnedByParent"
  | "shadowed"
  | "policyBlocked"
  | "sourceUnavailable"
  | "unknown";
export interface AgentAssetAction {
  action: AgentAssetActionKind;
  available: boolean;
  reason: AgentAssetActionUnavailableReason | null;
  mechanismId: string | null;
  confirmationRequired: boolean;
  reloadEffect: string | null;
  trustEffect: string | null;
  selectedInstallationId: string | null;
  risks: AgentAssetAccessRisk[];
}

export interface AgentHookRuleCount {
  agentKind: AgentCliKind;
  ruleCount: number | null;
}

export interface AgentEnvironmentInventory {
  environment: AgentEnvironmentDescriptor;
  installations: AgentInstallation[];
  sources: AgentAssetSource[];
  capabilities: AgentCapabilities[];
  assets: AgentAssetRecord[];
  hookRuleCounts: AgentHookRuleCount[];
  scannedAt: string;
  workspace: string | null;
  contexts: AgentConfigurationContext[];
  declarations: AgentAssetDeclaration[];
  limits: AgentAssetLimits;
  diagnostics: AgentAssetDiagnostic[];
  mechanisms: AgentAssetMechanismRecord[];
}

export interface AgentAssetDeclaration {
  id: string;
  contextId: string;
  sourceId: string;
  scope: AgentAssetScope;
  nativeKind: AgentAssetCategory;
  nativeId: string;
  declarationKey: string;
  label: string;
  precedence: number;
  presence: AgentAssetPresence;
  declaredState: AgentAssetDeclaredState;
  trustState: AgentTrustState;
  role: AgentAssetDeclarationRole;
  participation: AgentAssetResolutionParticipation;
  evidence: {
    revision: AgentAssetRevision;
    parserVersion: number;
    observedAt: string;
    facts: Record<string, string>;
  };
  diagnostics: AgentAssetDiagnostic[];
  providedBy: AgentAssetNativeRef | null;
  actionOwner: AgentAssetNativeRef | null;
  explicitlyAffected: AgentAssetNativeRef[];
}
export interface AgentAssetLimits {
  candidatePathsPerAgent: number;
  installationsPerAgent: number;
  sourcesPerContext: number;
  firstLevelEntries: number;
  bytesPerSource: number;
  bytesPerRefresh: number;
  refreshBudgetMs: number;
  cliOutputBytes: number;
  cliConcurrency: number;
  watchers: number;
  diagnostics: number;
}
export interface AgentAssetMechanismRecord {
  id: string;
  agentKind: AgentCliKind;
  category: AgentAssetCategory;
  action: AgentAssetActionKind;
  platforms: AgentHostPlatform[];
  adapterSchemaVersion: number;
  sourceSchema: string | null;
  executableArgv: string[];
  scopes: AgentAssetScope[];
  inspection: string;
  idempotent: boolean;
  commitPoint: string;
  reloadEffect: string | null;
  redactionRules: string[];
}

export interface AgentNativeTarget {
  platform: AgentHostPlatform;
  architecture: AgentHostArchitecture;
}
export type AgentVersionIdentifier =
  | { kind: "numeric"; value: number }
  | { kind: "text"; value: string };
export interface AgentSemanticVersion {
  major: number;
  minor: number;
  patch: number;
  prerelease: AgentVersionIdentifier[];
}
export type AgentAssetAccessUnavailableReason =
  "sourceUnavailable" | "snapshotUnavailable" | "policyUnavailable" | "unsupportedPlatform";
export type AgentAssetAccess =
  | { kind: "unavailable"; reason: AgentAssetAccessUnavailableReason }
  | { kind: "ready"; accessId: string };
export type AgentAssetAccessRisk = "externalPathnameRace" | "rawSensitiveContent";
export type AgentAssetAccessErrorKind =
  | "accessExpired" | "actorMismatch" | "targetMismatch" | "environmentMismatch"
  | "workspaceMismatch" | "rootChanged" | "sourceChanged" | "schemaChanged"
  | "policyUnavailable" | "outsideAllowedRoot" | "symlinkRejected"
  | "confirmationRequired" | "externalOpenFailed" | "unsupportedPlatform"
  | "accessUnavailable" | "readFailed";
export interface AgentAssetAccessError {
  kind: AgentAssetAccessErrorKind;
  message: string;
}
export interface AgentAssetReadRequest {
  targetId: string;
  accessId: string;
  environmentId: string;
  workspace: string | null;
}
export interface AgentAssetOpenRequest extends AgentAssetReadRequest {
  target: AgentAssetOpenTarget;
  acceptedRisks: AgentAssetAccessRisk[];
}
export interface AgentAssetPlanRequest {
  assetId: string;
  action: AgentAssetActionKind;
  workspace: string | null;
  expectedRevision: string;
  installationId: string | null;
}
export interface AgentAssetApplyRequest {
  planToken: string;
  assetId: string;
  action: AgentAssetActionKind;
}
export interface AgentAssetPlanChange {
  label: string;
  path: string | null;
  before: string | null;
  after: string | null;
}
export interface AgentAssetPlan {
  token: string;
  assetId: string;
  action: AgentAssetActionKind;
  title: string;
  mechanismId: string;
  selectedInstallationId: string | null;
  expiresAt: string;
  changes: AgentAssetPlanChange[];
  affectedAssetIds: string[];
  affectedInstallationIds: string[];
  sourceIds: string[];
  reloadEffect: string | null;
  trustEffect: string | null;
}
export type AgentAssetOperationPhase =
  "preparing" | "waitingForLock" | "revalidating" | "applying" | "verifying" | "completed";
export type AgentAssetOperationOutcome =
  | "canceledBeforeCommit" | "unchangedConflict" | "unchangedFailure"
  | "appliedVerified" | "appliedUnverified" | "outcomeUnknown";
export interface AgentAssetOperation {
  id: string;
  assetId: string;
  action: AgentAssetActionKind;
  phase: AgentAssetOperationPhase;
  canCancel: boolean;
  revision: number;
  createdAt: string;
  updatedAt: string;
  outcome: AgentAssetOperationOutcome | null;
  message: string | null;
  affectedAssetIds: string[];
  reloadEffect: string | null;
}
export type AgentAssetMutationErrorKind =
  | "invalidRequest" | "planExpired" | "planConsumed" | "actorMismatch"
  | "targetMismatch" | "actionMismatch" | "operationNotFound" | "sourceConflict"
  | "actionUnavailable" | "preparationFailed" | "capacityExceeded" | "internalFailure";
export interface AgentAssetMutationError {
  kind: AgentAssetMutationErrorKind;
  message: string;
  reason: AgentAssetActionUnavailableReason | null;
}

export interface AgentAssetReadResult {
  stableId: string;
  accessId: string;
  sourceRevision: AgentAssetRevision;
  path: string;
  content: string | null;
  sizeBytes: number;
  modifiedAt: string | null;
  truncated: boolean;
  metadataOnly: boolean;
  diagnostics: AgentAssetReadDiagnostic[];
}

export type AgentAssetReadDiagnostic =
  | "directoryMetadataOnly"
  | "sensitiveFileMetadataOnly"
  | "sensitiveValuesRedacted"
  | "unsupportedSchemaMetadataOnly"
  | "invalidDocumentMetadataOnly"
  | "readLimitMetadataOnly";

export type AgentRuntimeScope =
  { kind: "native" } | { kind: "wsl"; distro_id: string };
export type AgentHookMutation = "install" | "remove" | "enable" | "disable";
export type AgentHookActionKind =
  AgentHookMutation | "health" | "verify" | "repair";
export type AgentHookTrust =
  "unknown" | "trusted" | "required" | "not_applicable";
export type AgentHookHealthState =
  | "not_installed"
  | "installed_untrusted"
  | "installed_unverified"
  | "healthy"
  | "disabled"
  | "conflict"
  | "helper_missing"
  | "spool_blocked"
  | "unsupported";
export type AgentHookChangeKind = "add" | "remove" | "keep";

export interface AgentHookChange {
  eventName: string;
  structuralIdentity: string;
  fingerprint: string;
  kind: AgentHookChangeKind;
}

export interface AgentHookOwnedResource {
  eventName: string;
  structuralIdentity: string;
  contentFingerprint: string;
}

export interface AgentHookOwnership {
  agentKind: AgentCliKind;
  runtimeScope: AgentRuntimeScope;
  configPath: string;
  helperVersion: string;
  installedAt: number;
  enabled: boolean;
  resources: AgentHookOwnedResource[];
}

export interface AgentHookInspection {
  agentKind: AgentCliKind;
  runtimeScope: AgentRuntimeScope;
  configPath: string;
  configExists: boolean;
  revision: string;
  state: AgentHookHealthState;
  installed: boolean;
  enabled: boolean;
  trusted: AgentHookTrust;
  helperAvailable: boolean;
  spoolAvailable: boolean;
  lastEventAt: number | null;
  ownership: AgentHookOwnership | null;
  diagnostics: string[];
  actions: AgentHookAction[];
}

export interface AgentHookAction {
  action: AgentHookActionKind;
  available: boolean;
  reason: string | null;
}

export interface AgentHookPlan {
  agentKind: AgentCliKind;
  mutation: AgentHookMutation;
  runtimeScope: AgentRuntimeScope;
  configPath: string;
  expectedRevision: string;
  supported: boolean;
  conflict: boolean;
  changes: AgentHookChange[];
  contentChanges: AgentAssetPlanChange[];
  summary: string;
}

export type AgentRuntimeOrigin = "balancehub_launch" | "external_hook";
export type AgentRuntimeState =
  "starting" | "busy" | "idle" | "ended" | "unknown";
export type AgentRuntimeEvidenceSource =
  | "launch_registration"
  | "launch_status"
  | "hook"
  | "process"
  | "terminal"
  | "session_adapter";
export type AgentRuntimeConfidence = "weak" | "observed" | "exact";

export interface AgentRuntimeEvidence {
  source: AgentRuntimeEvidenceSource;
  confidence: AgentRuntimeConfidence;
  observedAt: number;
  eventId: string;
}

export interface AgentRuntimeProviderRef {
  providerId: string;
  providerName: string;
  accountLabel: string;
  apiKeyLocalId: string | null;
}

export interface AgentRuntimeProcessEvidence {
  pid: number;
  observedAt: number;
}

export interface AgentRuntimeTerminalEvidence {
  kind: TemporaryCliTerminalKind;
  locator: string | null;
  observedAt: number;
}

export interface AgentRuntimeActions {
  canActivateTerminal: boolean;
  canViewDetail: boolean;
  canResume: boolean;
  canDismiss: boolean;
}

export interface AgentRuntimeSession {
  runtimeId: string;
  nativeSession: AgentSessionLaunchIdentity | null;
  runtimeScope: AgentRuntimeScope;
  origin: AgentRuntimeOrigin;
  agentKind: AgentCliKind;
  agentSessionId: string | null;
  balancehubInstanceId: string | null;
  provider: AgentRuntimeProviderRef | null;
  workdir: string | null;
  title: string | null;
  model: string | null;
  process: AgentRuntimeProcessEvidence | null;
  terminal: AgentRuntimeTerminalEvidence | null;
  state: AgentRuntimeState;
  evidence: AgentRuntimeEvidence[];
  startedAt: number | null;
  lastActivityAt: number | null;
  endedAt: number | null;
  exitCode: number | null;
  actions: AgentRuntimeActions;
}

export interface AgentRuntimeSnapshot {
  schemaVersion: number;
  revision: number;
  updatedAt: number;
  sessions: AgentRuntimeSession[];
}

export interface TerminalEnvironmentProbeResult {
  terminals: TemporaryTerminalProbeResult[];
}

export interface ProviderModelSyncResult {
  provider: Provider;
  models: string[];
  message: string;
}

export interface ProviderAccountMutationResult {
  provider: Provider;
  message: string;
}

export interface ProviderCredentialCompletionStep {
  name: string;
  ok: boolean;
  message: string;
}

export interface ProviderCredentialCompletionResult {
  input: ProviderInput;
  changedFields: string[];
  steps: ProviderCredentialCompletionStep[];
  apiKeyOptions: ProviderApiKeyOption[];
}

export interface ProviderApiKeyOption {
  localId: string;
  /** BalanceHub 本地备注；远端同步不得覆盖。 */
  localName: string;
  /** 站点返回的远程 Key 名称。 */
  name: string;
  key: string;
  maskedKey: string;
  keyAvailable: boolean;
  tokenId: string;
  userId: string;
  status: string;
  usedQuota: number;
  remainQuota: number;
  usedQuotaRaw: number;
  remainQuotaRaw: number;
  unlimitedQuota: boolean;
  group: string;
  totalQuota: number;
  groupId: string;
  crossGroupRetry: boolean;
  modelLimitsEnabled: boolean;
  modelLimits: string[];
  allowIps: string[];
  denyIps: string[];
  autoGroups: string[];
  spendingLimits: ProviderApiKeySpendingLimits;
  quotaDisplayType: string;
  currencySymbol: string;
  createdTime?: number | null;
  accessedTime?: number | null;
  expiredTime?: number | null;
}

export interface ProviderApiKeySpendingLimits {
  fiveHours: number;
  oneDay: number;
  sevenDays: number;
}

export type ProviderApiKeyExpiration =
  | { mode: "never" }
  | { mode: "at"; timestamp: number }
  | { mode: "afterDays"; days: number };

export interface ProviderApiKeySettings {
  name: string;
  group: string;
  quota: { unlimited: boolean; amount: number };
  expiration: ProviderApiKeyExpiration;
  allowIps: string[];
  denyIps: string[];
  modelLimits: string[];
  modelLimitsEnabled: boolean;
  crossGroupRetry: boolean;
  autoGroups: string[];
  spendingLimits: ProviderApiKeySpendingLimits;
  enabled: boolean;
}

export type ProviderApiKeyPatch = Partial<ProviderApiKeySettings> & { customKey?: string };

export interface ProviderApiKeyEditorContext {
  credentialRevision: number;
  settings: ProviderApiKeySettings;
  groups: { value: string; label: string; description: string; rate: number | null }[];
  groupClearable: boolean;
  defaultGroupLabel: string;
  quotaLabel: string;
  quotaUnit: string;
  quotaMinimum: number;
  expirationInDays: boolean;
  supportsIpBlacklist: boolean;
  supportsModelLimits: boolean;
  supportsCrossGroupRetry: boolean;
  automaticGroup: string | null;
  supportsSpendingLimits: boolean;
  supportsCustomKey: boolean;
  autoGroups: { groups: string[]; maxCount: number } | null;
  modelOptions: string[];
  modelOptionsError: string | null;
}

export interface ProviderConnectionTestResult {
  ok: boolean;
  message: string;
  available: number | null;
  used: number | null;
  quotaDisplay: ProviderQuotaDisplay;
  steps: ProviderConnectionTestStep[];
}

export interface ProviderConnectionTestStep {
  name: string;
  ok: boolean;
  message: string;
  available: number | null;
  used: number | null;
  quotaDisplay: ProviderQuotaDisplay;
}

export interface ProviderQuotaDisplay {
  quotaDisplayType: string;
  currencySymbol: string;
}

export interface ProviderUsagePoint {
  date: string;
  used: number;
  requestCount: number;
  tokenUsed: number;
}

export interface ProviderUsageModelStat {
  modelName: string;
  used: number;
  requestCount: number;
  tokenUsed: number;
}

export interface ProviderUsageModelPoint {
  date: string;
  modelName: string;
  used: number;
  requestCount: number;
  tokenUsed: number;
}

export interface ProviderModelHealth {
  modelName: string;
  successRate: number | null;
  avgLatencyMs: number | null;
  avgTps: number | null;
}

export interface ProviderUsageSummary {
  providerId: string;
  providerName: string;
  quotaDisplay: ProviderQuotaDisplay;
  points: ProviderUsagePoint[];
  modelStats: ProviderUsageModelStat[];
  modelPoints: ProviderUsageModelPoint[];
  /** 站点没有性能采样能力时为空数组。 */
  modelHealth: ProviderModelHealth[];
  /** 周期内请求总数；站点只给前三站模型明细时仍由这里给出完整次数。 */
  requestCount: number;
  /** 本次统计实际覆盖的分钟数；今日视图是从本地零点到现在。 */
  windowMinutes: number;
}

/** 顶栏的全局用量趋势：多个站点合并结果，因此没有单一配额单位。 */
export interface ProviderUsageOverview {
  requestCount: number;
  modelStats: ProviderUsageModelStat[];
  /** 合并去重后的模型总数；modelStats 只保留排行前几项。 */
  modelCount: number;
  modelHealth: ProviderModelHealth[];
  /** 读取失败的站点；单个站点失败不影响其余站点的汇总。 */
  errors: ProviderUsageSourceError[];
  /** 各站实际窗口的最大值，用于计算汇总视图的平均 RPM/TPM。 */
  windowMinutes: number;
}

export interface ProviderUsageSourceError {
  providerId: string;
  providerName: string;
  message: string;
}

export type ProviderRequestLogTimeRange = "24h" | "7d" | "30d" | "90d";

export interface ProviderRequestLogFilters {
  model: string;
  tokenName: string;
  group: string;
  requestId: string;
  logType: number | null;
  range: ProviderRequestLogTimeRange;
}

export interface ProviderRequestLogsQuery {
  model: string;
  tokenName: string;
  group: string;
  requestId: string;
  logType: number | null;
  startTimestamp: number | null;
  endTimestamp: number | null;
  page: number;
  pageSize: number;
}

export interface ProviderRequestLog {
  id: string;
  createdAt: string;
  tokenName: string;
  modelName: string;
  requestId: string;
  status: string;
  promptTokens: number;
  completionTokens: number;
  tokenUsed: number;
  quota: number;
  channel: string;
  durationMs?: number | null;
  content: string;
  raw: Record<string, unknown>;
}

export interface ProviderRequestLogStats {
  /** 对应统计接口读取失败时为 null，界面显示「—」而不是 0。 */
  quota: number | null;
  rpm: number | null;
  tpm: number | null;
  /** 站点不提供平均耗时统计时为 null。 */
  avgDurationMs: number | null;
}

export interface ProviderRequestLogsResult {
  providerId: string;
  providerName: string;
  page: number;
  pageSize: number;
  total?: number | null;
  quotaDisplay: ProviderQuotaDisplay;
  stats: ProviderRequestLogStats;
  logs: ProviderRequestLog[];
  message: string;
}

export interface ProviderCheckInRecord {
  date: string;
  checkedAt?: string | null;
  quotaDelta?: number | null;
  message: string;
}

export interface ProviderCheckInRecordsResult {
  providerId: string;
  month: string;
  records: ProviderCheckInRecord[];
  quotaDisplay: ProviderQuotaDisplay;
  message: string;
}

export interface ProviderCapabilityProbeResult {
  provider: Provider;
  message: string;
}

export interface ProviderSiteProbeResult {
  ok: boolean;
  message: string;
  systemName: string | null;
  logo: string | null;
  quotaDisplay: ProviderQuotaDisplay;
}

export interface ProviderProtocolDetectionResult {
  detectedProtocol: ProviderProtocol | null;
  message: string;
  site: ProviderSiteProbeResult | null;
  ambiguous: boolean;
}

export interface CliConfigSnapshot {
  cliKind: AgentCliKind;
  configured: boolean;
  providerId: string | null;
  apiKeyLocalId: string | null;
  modifiedAt: string | null;
  errorMessage: string | null;
}

export interface TemporaryCliInstance {
  id: string;
  providerId: string | null;
  providerName: string | null;
  nativeSession: AgentSessionLaunchIdentity | null;
  apiKeyLocalId?: string | null;
  sessionTitle: string;
  accountLabel: string;
  cliKind: AgentCliKind;
  workdir: string;
  terminalKind: TemporaryCliTerminalKind;
  terminalName: string;
  terminalLocator?: string | null;
  startedAt: string;
  endedAt: string | null;
  pid: number | null;
  status: TemporaryCliInstanceStatus;
  exitCode: number | null;
  canActivate: boolean;
}

export interface Workspace {
  path: string;
  useCount: number;
}

export interface TemporaryCliPreference {
  providerId: string;
  cliKind: AgentCliKind;
  apiKeyLocalId: string;
  model: string;
  workspacePath: string;
}

export interface TemporaryCliLaunchInput {
  inheritedModel?: string;
  runSettings?: import("./temporary-cli-settings-types").TemporaryCliRunSettings;
  providerId: string;
  cliKind: AgentCliKind;
  cliPath: string;
  workdir: string;
  apiKey: string;
  apiKeyLocalId: string;
  model: string;
  sessionMode: TemporaryCliSessionMode;
  sessionName: string;
  resumeId: string;
  sessionTitle: string;
  terminalKind: TemporaryCliTerminalKind;
}

/**
 * 打开临时启动选择器时的预置上下文。交接功能用它把目标目录和交接文件说明
 * 带进现有启动流程，不在启动器里再实现一套启动逻辑。
 */
export interface TemporaryCliLaunchContext {
  workdir?: string;
  initialPrompt?: string;
}

export interface TemporaryCliLaunchPreview {
  modelSelection: import("./agent-settings-types").AgentSettingValueSummary;
  runSummary: string[];
  providerName: string;
  cliKind: AgentCliKind;
  cliPath: string;
  args: string[];
  command: string;
  terminalKind: TemporaryCliTerminalKind;
  terminalName: string;
  workdir: string;
  baseUrl: string;
  apiKeyLabel: string;
  apiKey: string;
  model: string;
  sessionMode: TemporaryCliSessionMode;
  sessionName: string;
  resumeId: string;
  environment: Record<string, string>;
  settingsPath: string | null;
  settingsContent: string | null;
}

export interface CliSessionSummary {
  id: string;
  title: string;
  preview: string | null;
  model: string | null;
  models: string[];
  cliKind: AgentCliKind;
  createdAt: string | null;
  updatedAt: string | null;
  workdir: string;
  cliVersion: string | null;
  archived: boolean;
  canResume: boolean;
  metadataSource: string;
}

export type CliSessionMessageRole = "user" | "assistant" | "tool";

export interface CliSessionMessage {
  contentVersion: number;
  metadata: SessionMessageMetadata;
  contentLength: number;
  contentOffset: number;
  hasMoreContent: boolean;
  changes: SessionFileChange[];
  id: string;
  role: CliSessionMessageRole;
  content: string;
  timestamp: string | null;
  model: string | null;
  toolName: string | null;
}

export interface CliSessionDetail {
  usage: import("./session-usage-types").SessionUsageSummary;
  session: CliSessionSummary;
  messages: CliSessionMessage[];
  page: SessionReadPage;
  outline: SessionTurnSummary[];
  find: SessionFindState;
}

export interface WorkspaceDirectoryEntry {
  name: string;
  path: string;
  hidden: boolean;
}

export interface WorkspaceDirectoryListing {
  currentPath: string;
  parentPath: string | null;
  homePath: string;
  entries: WorkspaceDirectoryEntry[];
}

export interface TemporaryCliLaunchResult {
  instance: TemporaryCliInstance;
  workspaces: Workspace[];
  workspaceError: string | null;
  preference: TemporaryCliPreference;
}

export interface CliRuntimeSnapshot {
  agents: AgentCliDescriptor[];
  configs: CliConfigSnapshot[];
}

/** 公告在站点上属于哪一类来源：NewAPI 站点总公告 / 控制台公告，Sub2API 公告。 */
export type SiteAnnouncementSource = "siteNotice" | "console" | "announcement";

export interface SiteAnnouncement {
  id: string;
  fingerprint: string;
  providerId: string;
  providerName: string;
  providerProtocol: ProviderProtocol;
  source: SiteAnnouncementSource;
  title: string;
  /** 标题由后端从正文第一行提取：只用于本地识别，界面上不再单独渲染。 */
  titleIsDerived: boolean;
  content: string;
  /** 协议给出的公告级别（NewAPI: default/ongoing/success/warning/error）。 */
  level: string;
  /** 公告正文之外的附加说明。 */
  extra: string;
  publishedAt: string | null;
  updatedAt: string | null;
  readAt: string | null;
  canMarkRead: boolean;
}

export interface SiteAnnouncementSourceError {
  providerId: string;
  providerName: string;
  providerProtocol: ProviderProtocol;
  message: string;
}

export interface SiteAnnouncementsSnapshot {
  fetchedAt: string;
  announcements: SiteAnnouncement[];
  errors: SiteAnnouncementSourceError[];
  /** 公告条数触到单站上限的站点名；列表只包含上限内的部分。 */
  truncatedSources: string[];
}

export interface AppSettings {
  onboardingCompleted: boolean;
  refreshInterval: number;
  launchAtLogin: boolean;
  launchAtLoginMinimized: boolean;
  proxyMode: ProxyMode;
  proxyUrl: string;
  themeMode: ThemeMode;
  autoRefreshEnabled: boolean;
  autoCheckInEnabled: boolean;
  checkInTime: string;
  notificationEnabled: boolean;
  notificationChannels: NotificationChannel[];
  livenessCliKind: AgentCliKind;
  agentCliPaths: Partial<Record<AgentCliKind, string>>;
  temporaryCliTerminalKind: TemporaryCliTerminalKind;
  livenessEnabled: boolean;
  livenessModel: string;
  livenessIntervalMode: LivenessIntervalMode;
  livenessInterval: number;
  livenessRandomMinInterval: number;
  livenessRandomMaxInterval: number;
  livenessTimeout: number;
  livenessPromptMode: LivenessPromptMode;
  livenessFixedPrompt: string;
  livenessPromptLibrary: string[];
  livenessPlaceholderPools: LivenessPlaceholderPool[];
  livenessNumberMin: number;
  livenessNumberMax: number;
  windowCloseAction: WindowCloseAction;
  windowHideDockIcon: boolean;
  windowTrayClickAction: TrayClickAction;
  windowRestoreState: boolean;
  windowCloseHintShown: boolean;
  updateAutoCheck: boolean;
  updateCheckIntervalHours: number;
  notifyRefreshFailure: boolean;
  notifyLivenessFailure: boolean;
  notifyCheckIn: boolean;
  notifyQuietHours: boolean;
  notifyQuietStart: string;
  notifyQuietEnd: string;
  notifyRetryCount: number;
  notifyMergeWindowSecs: number;
  refreshRetryCount: number;
  refreshJitter: boolean;
  livenessFailureThreshold: number;
  livenessRecordLimit: number;
  checkInRecordLimit: number;
  httpTimeoutSecs: number;
  proxyBypass: string;
  logLevel: LogLevel;
  logRetentionDays: number;
}

export interface LivenessPlaceholderPool {
  key: string;
  values: string[];
}

export interface NotificationChannel {
  id: string;
  name: string;
  kind: NotificationChannelKind;
  url: string;
  secret: string;
  enabled: boolean;
}
