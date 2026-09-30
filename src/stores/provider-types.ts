import type { AgentCliKind } from "../agent-cli/visuals.ts";
import type { AgentSessionLaunchIdentity } from "./agent-session-types";

export type { AgentCliKind } from "../agent-cli/visuals.ts";
export type * from "./agent-catalog-types";
export type * from "./agent-lifecycle-types";
export type * from "./agent-session-types";

export type AuthMode = "apiKey" | "accessToken" | "session" | "password";
export type AuthSource = "manual" | "password" | "oauth";
export type ProviderProtocol = "newApi" | "sub2Api" | "api";

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
  accessTokenSkipFields: string[];
  apiKeyRequiredFields: string[];
  apiKeyRequiredAnyFields: string[];
}

export interface ProviderProtocolDescriptor {
  kind: ProviderProtocol;
  label: string;
  description: string;
  defaultAuthMode: AuthMode;
  browserLoginSupported: boolean;
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
  id: string;
  role: CliSessionMessageRole;
  content: string;
  timestamp: string | null;
  model: string | null;
  toolName: string | null;
}

export interface CliSessionDetail {
  session: CliSessionSummary;
  messages: CliSessionMessage[];
  truncated: boolean;
  omittedMessageCount: number;
  contentSource: string;
}

export type CliSessionIndexState =
  "ready" | "disabled" | "fallback";

export interface CliSessionIndexAgentStats {
  cliKind: AgentCliKind;
  sizeBytes: number;
  sessionCount: number;
  updatedAt: string | null;
}

export interface CliSessionIndexStatus {
  enabled: boolean;
  directory: string;
  maxSizeMiB: number;
  sizeBytes: number;
  building: boolean;
  agents: CliSessionIndexAgentStats[];
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
  sessionIndexEnabled: boolean;
  sessionIndexDirectory: string;
  sessionIndexMaxSizeMiB: number;
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
