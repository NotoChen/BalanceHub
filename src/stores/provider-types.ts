import type { AgentCliKind } from "../agent-cli/visuals.ts";

export type { AgentCliKind } from "../agent-cli/visuals.ts";

export type AuthMode = "apiKey" | "accessToken" | "session" | "password";
export type AuthSource = "manual" | "password" | "oauth";
export type ProviderProtocol = "newApi" | "sub2Api" | "api";

export interface ProviderAuthModeDescriptor {
  mode: AuthMode;
  label: string;
  description: string;
  note: string;
  requiredFields: string[];
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
  checkIn: boolean;
  announcements: boolean;
}

export interface ProviderProtocolOperationMethodsDescriptor {
  checkIn: string | null;
  apiKeys: string | null;
  invitation: string | null;
  models: string;
  announcements: string | null;
}

export interface ProviderCredentialAssistantDescriptor {
  enabled: boolean;
  accessTokenFlow: "none" | "credentialCompletion" | "sessionGeneration";
  apiKeyRequiredFields: string[];
  apiKeyRequiredAnyFields: string[];
}

export interface ProviderProtocolDescriptor {
  kind: ProviderProtocol;
  label: string;
  description: string;
  defaultAuthMode: AuthMode;
  authModes: ProviderAuthModeDescriptor[];
  capabilities: ProviderProtocolCapabilitiesDescriptor;
  operationMethods: ProviderProtocolOperationMethodsDescriptor;
  credentialAssistant: ProviderCredentialAssistantDescriptor;
}
export type ProviderQuotaScope = "account" | "token";
export type ProviderStatus = "ok" | "warning" | "error" | "syncing";
export type ProxyMode = "system" | "noProxy" | "custom";
export type ProviderProxyMode = "inherit" | "system" | "noProxy" | "custom";
export type ProviderNotificationMode = "inherit" | "custom" | "disabled";
export type ThemeMode = "system" | "light" | "dark";
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
  | "system"
  | "dingtalk"
  | "wecom"
  | "feishu"
  | "slack"
  | "generic";

export interface Provider {
  revision: number;
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
  actions: ProviderActions;
}

export interface ProviderActions {
  accountManagement: boolean;
  checkIn: boolean;
  checkedInToday: boolean;
  apiKeyManagement: boolean;
  invitation: boolean;
  refreshModelsOnly: boolean;
}

export interface ProviderIdentity {
  id: string;
  name: string;
  baseUrl: string;
  protocol: ProviderProtocol;
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
  userId: string;
  backupUrls: string[];
}

export interface ProviderCli {
  preferredModel: string;
}

export interface ProviderCliInput {
  preferredModel: string;
}

export interface ProviderAuth {
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

export interface ProviderRuntime {
  enabled: boolean;
  status: ProviderStatus;
  errorMessage?: string | null;
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
  runtime: Pick<ProviderRuntime, "enabled">;
}

export type ProviderSaveConflictKind =
  | "sameAccount"
  | "sameApiKey"
  | "sameUrlDifferentApiKey";

export interface ProviderSaveOptions {
  overwriteProviderId?: string;
  mergeApiKeyIntoProviderId?: string;
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
  sessionResume: boolean;
  sessionName: boolean;
  liveness: boolean;
  defaultConfig: boolean;
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

export interface TerminalEnvironmentProbeResult {
  terminals: TemporaryTerminalProbeResult[];
}

export interface ProviderModelSyncResult {
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
  crossGroupRetry: boolean;
  modelLimitsEnabled: boolean;
  modelLimits: string[];
  allowIps: string[];
  quotaDisplayType: string;
  currencySymbol: string;
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
  modifiedAt: string | null;
  errorMessage: string | null;
}

export interface CliConfigFile {
  filePath: string;
  content: string;
}

export interface CliConfigPreview {
  providerId: string;
  providerName: string;
  cliKind: AgentCliKind;
  revision: string;
  originalFiles: CliConfigFile[];
  files: CliConfigFile[];
}

export interface TemporaryCliInstance {
  id: string;
  providerId: string;
  providerName: string;
  sessionTitle: string;
  accountLabel: string;
  cliKind: AgentCliKind;
  workdir: string;
  terminalKind: TemporaryCliTerminalKind;
  terminalName: string;
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
  apiKeyTokenId: string;
  model: string;
  workspacePath: string;
}

export interface TemporaryCliLaunchInput {
  providerId: string;
  cliKind: AgentCliKind;
  cliPath: string;
  workdir: string;
  apiKey: string;
  apiKeyTokenId: string;
  model: string;
  sessionMode: TemporaryCliSessionMode;
  sessionName: string;
  resumeId: string;
  sessionTitle: string;
  terminalKind: TemporaryCliTerminalKind;
}

export interface TemporaryCliLaunchPreview {
  providerName: string;
  cliKind: AgentCliKind;
  cliPath: string;
  args: string[];
  command: string;
  terminalKind: TemporaryCliTerminalKind;
  terminalName: string;
  workdir: string;
  baseUrl: string;
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
  instances: TemporaryCliInstance[];
}

export interface SiteAnnouncement {
  id: string;
  fingerprint: string;
  providerId: string;
  providerName: string;
  providerProtocol: ProviderProtocol;
  title: string;
  content: string;
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
