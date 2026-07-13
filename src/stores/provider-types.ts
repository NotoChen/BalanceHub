export type AuthMode = "apiKey" | "accessToken" | "session";
export type ProviderQuotaScope = "account" | "token";
export type ProviderStatus = "ok" | "warning" | "error" | "syncing";
export type ProxyMode = "system" | "noProxy" | "custom";
export type ProviderProxyMode = "inherit" | "system" | "noProxy" | "custom";
export type ProviderNotificationMode = "inherit" | "custom" | "disabled";
export type ThemeMode = "system" | "light" | "dark";
export type LivenessIntervalMode = "fixed" | "random";
export type LivenessPromptMode = "fixed" | "random" | "roundRobin";
export type LivenessCliKind = "codex" | "claudeCode";
export type LivenessMethod = "cli" | "http";
export type LivenessHttpProtocol = "openaiChat" | "openaiResponses" | "anthropic";
export type TemporaryCliTerminalKind =
  | "auto"
  | "systemDefault"
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
  | "powerShell"
  | "custom";
export type NotificationChannelKind =
  | "system"
  | "dingtalk"
  | "wecom"
  | "feishu"
  | "slack"
  | "generic";

export interface Provider {
  identity: ProviderIdentity;
  auth: ProviderAuth;
  quota: ProviderQuota;
  capabilities: ProviderCapabilities;
  automation: ProviderAutomation;
  liveness: ProviderLiveness;
  proxy: ProviderProxy;
  notification: ProviderNotification;
  runtime: ProviderRuntime;
}

export interface ProviderIdentity {
  id: string;
  name: string;
  baseUrl: string;
  displayName: string;
  username: string;
  userId: string;
  siteLogo: string;
}

export interface ProviderIdentityInput {
  name: string;
  baseUrl: string;
}

export interface ProviderAuth {
  mode: AuthMode;
  apiKey: string;
  accessToken: string;
  sessionCookie: string;
  apiUser: string;
}

export interface ProviderQuota {
  available: number;
  used: number;
  scope?: ProviderQuotaScope;
  unlimited?: boolean;
  perUnit: number;
  displayType: string;
  currencySymbol: string;
  currencyExchangeRate: number;
}

export interface ProviderCapabilities {
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
}

export interface ProviderAutomation {
  refreshInterval: number;
  checkInTime: string;
  lastSyncedAt: string | null;
  lastCheckedInAt: string | null;
  lastCheckInUser: string;
  checkInRecords: ProviderCheckInRecord[];
}

export interface ProviderAutomationInput {
  refreshInterval: number;
  checkInTime: string;
}

export interface ProviderLiveness {
  useGlobal: boolean;
  enabled: boolean;
  openaiBaseUrl: string;
  anthropicBaseUrl: string;
  cliKind?: LivenessCliKind | null;
  method?: LivenessMethod | null;
  httpProtocol?: LivenessHttpProtocol | null;
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
  openaiBaseUrl: string;
  anthropicBaseUrl: string;
  cliKind?: LivenessCliKind | null;
  method?: LivenessMethod | null;
  httpProtocol?: LivenessHttpProtocol | null;
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

export interface ProviderRuntime {
  enabled: boolean;
  status: ProviderStatus;
  errorMessage?: string | null;
}

export interface ProviderInput {
  id?: string;
  identity: ProviderIdentityInput;
  auth: ProviderAuth;
  automation: ProviderAutomationInput;
  liveness: ProviderLivenessInput;
  proxy: ProviderProxy;
  notification: ProviderNotification;
  runtime: Pick<ProviderRuntime, "enabled">;
}

export interface LivenessRecord {
  checkedAt: string;
  source?: "manual" | "automatic" | string;
  cliKind?: LivenessCliKind | string;
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

export interface LivenessRunResult {
  providers: Provider[];
  provider: Provider;
  record: LivenessRecord;
  codexPath: string;
}

export interface CodexCliProbeResult {
  path: string;
  version: string;
}

export interface CliCandidate {
  path: string;
  version: string | null;
  valid: boolean;
  source: string;
  isPathDefault?: boolean;
}

export interface CodexModelSyncResult {
  providers: Provider[];
  provider: Provider;
  models: string[];
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
  name: string;
  key: string;
  tokenId: string;
  status: string;
  usedQuota: number;
  remainQuota: number;
  unlimitedQuota: boolean;
  group: string;
  modelLimitsEnabled: boolean;
  modelLimits: string[];
  allowIps: string[];
  createdTime?: number | null;
  accessedTime?: number | null;
  expiredTime?: number | null;
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

export interface ProviderUsageSummary {
  providerId: string;
  providerName: string;
  quotaDisplay: ProviderQuotaDisplay;
  points: ProviderUsagePoint[];
  modelStats: ProviderUsageModelStat[];
  modelPoints: ProviderUsageModelPoint[];
}

export interface ProviderRequestLogsQuery {
  keyword: string;
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
  quota: number;
  rpm: number;
  tpm: number;
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
  providers: Provider[];
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
  glassTransparency: number;
  livenessCliKind: LivenessCliKind;
  livenessMethod: LivenessMethod;
  livenessHttpProtocol: LivenessHttpProtocol;
  codexCliPath: string;
  claudeCliPath: string;
  temporaryCliTerminalKind: TemporaryCliTerminalKind;
  temporaryCliTerminalCommand: string;
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
  livenessConsentAcceptedAt: string | null;
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
