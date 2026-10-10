import { invoke, type Channel } from "@tauri-apps/api/core";
import type { AgentConfigurationEdit } from "../stores/agent-configuration-types";
import type {
  AppSettings,
  CliRuntimeSnapshot,
  CliSessionIndexStatus,
  CliEnvironmentProbeResult,
  TerminalEnvironmentProbeResult,
  ProviderModelSyncResult,
  AgentCliKind,
  Provider,
  ProviderProtocolDescriptor,
  ProviderApiKeyOption,
  ProviderApiKeyPatch,
  ProviderApiKeyEditorContext,
  ProviderCapabilityProbeResult,
  ProviderCheckInRecordsResult,
  ProviderCheckInPolicyPreview,
  ProviderCredentialCompletionResult,
  ProviderConnectionTestResult,
  ProviderInput,
  ProviderSaveOptions,
  ProviderSaveResult,
  ProviderProtocolDetectionResult,
  ProviderRemovalResult,
  ProviderRequestLogsQuery,
  ProviderRequestLogsResult,
  ProviderSiteProbeResult,
  ProviderUsageSummary,
  TemporaryCliInstance,
  TemporaryCliLaunchInput,
  TemporaryCliLaunchPreview,
  TemporaryCliLaunchResult,
  TemporaryCliPreference,
  SiteAnnouncementsSnapshot,
  Workspace,
  WorkspaceDirectoryListing,
  AgentEnvironmentInventory,
  AgentAssetReadResult,
  AgentAssetReadRequest,
  AgentAssetOpenRequest,
  AgentAssetOpenTarget,
  AgentAssetAccessRisk,
  AgentAssetPlanRequest,
  AgentAssetApplyRequest,
  AgentAssetPlan,
  AgentAssetOperation,
  AgentHookInspection,
  AgentHookMutation,
  AgentHookPlan,
  AgentRuntimeSnapshot,
} from "../stores/providers";

export interface AppData {
  revision: number;
  schemaVersion: number;
  providers: Provider[];
  providerProtocols: ProviderProtocolDescriptor[];
  settings: AppSettings;
  workspaces: Workspace[];
  temporaryCliPreferences: TemporaryCliPreference[];
  loginAccounts: import("../stores/provider-types").LoginAccount[];
}

export interface RefreshResult {
  updatedProviders: Provider[];
}

export interface NotificationDeliveryResult {
  channelId: string;
  channelName: string;
  channelKind: AppSettings["notificationChannels"][number]["kind"];
  ok: boolean;
  message: string;
}

export interface NotificationSendResult {
  sentCount: number;
  results: NotificationDeliveryResult[];
}

export interface AppDataTransferResult {
  path: string;
  schemaVersion: number;
  providerCount: number;
}

export interface AppDataImportResult {
  data: AppData;
  transfer: AppDataTransferResult;
}

export interface AppUpdateInfo {
  currentVersion: string;
  version: string;
  date?: string | null;
  body?: string | null;
  rawJson?: Record<string, unknown> | null;
}

export type AppUpdateDownloadEvent =
  | { event: "Started"; data: { contentLength?: number | null } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Verifying" }
  | { event: "Installing" }
  | { event: "Finished" };

export function loadAppData() {
  return invoke<AppData>("load_app_data");
}

export function hostPlatform() {
  return invoke<string>("host_platform");
}

export function openCcSwitchDeeplink(url: string) {
  return invoke<void>("open_ccswitch_deeplink", { url });
}

export function openProjectRepository() {
  return invoke<void>("open_project_repository");
}

export function checkAppUpdate() {
  return invoke<AppUpdateInfo | null>("check_app_update");
}

export function installAppUpdate(onEvent: Channel<AppUpdateDownloadEvent>) {
  return invoke<void>("install_app_update", { onEvent });
}

export function cancelAppUpdate() {
  return invoke<void>("cancel_app_update");
}

export function clearPendingAppUpdate() {
  return invoke<void>("clear_pending_app_update");
}

export function cancelVisibleRelaunch() {
  return invoke<void>("cancel_visible_relaunch");
}

export function saveProvider(input: ProviderInput, options: ProviderSaveOptions = {}) {
  return invoke<ProviderSaveResult>("save_provider", { input, options });
}

export function previewProviderCheckInPolicy(input: ProviderInput) {
  return invoke<ProviderCheckInPolicyPreview>("preview_provider_check_in_policy", { input });
}

export function removeProvider(id: string) {
  return invoke<ProviderRemovalResult>("remove_provider", { id });
}

export function reorderProviders(ids: string[]) {
  return invoke<string[]>("reorder_providers", { ids });
}

export function saveSettings(settings: AppSettings, expected: AppSettings) {
  return invoke<AppSettings>("save_settings", { settings, expected });
}

export function sendAppNotification(
  settings: AppSettings,
  title: string,
  markdown: string,
  ignoreSwitch = false,
  provider?: Provider,
) {
  return invoke<NotificationSendResult>("send_app_notification", {
    settings,
    provider: provider ?? null,
    title,
    markdown,
    ignoreSwitch,
  });
}

export function exportAppData(path: string) {
  return invoke<AppDataTransferResult>("export_app_data", { path });
}

export function importAppData(path: string) {
  return invoke<AppDataImportResult>("import_app_data", { path });
}

export function completeProviderCredentials(input: ProviderInput) {
  return invoke<ProviderCredentialCompletionResult>("complete_provider_credentials", { input });
}

export function probeProviderSite(input: ProviderInput) {
  return invoke<ProviderSiteProbeResult>("probe_provider_site", { input });
}

export function detectProviderProtocol(input: ProviderInput) {
  return invoke<ProviderProtocolDetectionResult>("detect_provider_protocol", { input });
}

export function testProviderConnection(input: ProviderInput) {
  return invoke<ProviderConnectionTestResult>("test_provider_connection", { input });
}

export function probeCliTools(deep = false) {
  return invoke<CliEnvironmentProbeResult>("probe_cli_tools", { deep });
}

export function getCachedCliTools() {
  return invoke<CliEnvironmentProbeResult | null>("get_cached_cli_tools");
}

export function probeTerminals() {
  return invoke<TerminalEnvironmentProbeResult>("probe_terminals");
}

export function getAgentEnvironmentInventory(workspace?: string) {
  return invoke<AgentEnvironmentInventory>("get_agent_environment_inventory", {
    workspace: workspace || null,
  });
}

export interface AgentEnvironmentAccessInput {
  accessId: string;
  environmentId: string;
  workspace?: string;
}

export function readAgentEnvironmentAsset(assetId: string, access: AgentEnvironmentAccessInput) {
  return invoke<AgentAssetReadResult>("read_agent_environment_asset", {
    request: { targetId: assetId, ...access, workspace: access.workspace || null } satisfies AgentAssetReadRequest,
  });
}

export function openAgentEnvironmentAsset(
  assetId: string,
  access: AgentEnvironmentAccessInput,
  target: AgentAssetOpenTarget = "asset",
  acceptedRisks: AgentAssetAccessRisk[] = [],
) {
  return invoke<void>("open_agent_environment_asset", {
    request: { targetId: assetId, ...access, workspace: access.workspace || null, target, acceptedRisks } satisfies AgentAssetOpenRequest,
  });
}

export function readAgentEnvironmentSource(sourceId: string, access: AgentEnvironmentAccessInput) {
  return invoke<AgentAssetReadResult>("read_agent_environment_source", {
    request: { targetId: sourceId, ...access, workspace: access.workspace || null } satisfies AgentAssetReadRequest,
  });
}

export function openAgentEnvironmentSource(
  sourceId: string,
  access: AgentEnvironmentAccessInput,
  target: AgentAssetOpenTarget = "asset",
  acceptedRisks: AgentAssetAccessRisk[] = [],
) {
  return invoke<void>("open_agent_environment_source", {
    request: { targetId: sourceId, ...access, workspace: access.workspace || null, target, acceptedRisks } satisfies AgentAssetOpenRequest,
  });
}

export function planAgentAsset(request: AgentAssetPlanRequest, requestId: string, agentKind: AgentCliKind) {
  return invoke<AgentAssetPlan>("plan_agent_asset", { request, requestId, agentKind });
}

export function applyAgentAsset(request: AgentAssetApplyRequest) {
  return invoke<AgentAssetOperation>("apply_agent_asset", { request });
}

export function getAgentAssetOperation(operationId: string) {
  return invoke<AgentAssetOperation>("get_agent_asset_operation", { operationId });
}

export function cancelAgentAssetOperation(operationId: string) {
  return invoke<AgentAssetOperation>("cancel_agent_asset_operation", { operationId });
}

export function listAgentAssetOperations() {
  return invoke<AgentAssetOperation[]>("list_agent_asset_operations");
}

export function verifyAgentAssetOperation(operationId: string) {
  return invoke<AgentAssetOperation>("verify_agent_asset_operation", { operationId });
}

export function inspectAgentHook(agentKind: AgentCliKind) {
  return invoke<AgentHookInspection>("inspect_agent_hook", { agentKind });
}

export function planAgentHook(agentKind: AgentCliKind, mutation: AgentHookMutation) {
  return invoke<AgentHookPlan>("plan_agent_hook", { agentKind, mutation });
}

export function applyAgentHook(agentKind: AgentCliKind, plan: AgentHookPlan) {
  return invoke<AgentHookInspection>("apply_agent_hook", { agentKind, plan });
}

export function healthAgentHook(agentKind: AgentCliKind) {
  return invoke<AgentHookInspection>("health_agent_hook", { agentKind });
}

export function repairAgentHook(agentKind: AgentCliKind) {
  return invoke<AgentHookPlan>("repair_agent_hook", { agentKind });
}

export function verifyAgentHook(agentKind: AgentCliKind) {
  return invoke<AgentHookInspection>("verify_agent_hook", { agentKind });
}

export function previewLivenessPrompts(settings: AppSettings, count = 10) {
  return invoke<string[]>("preview_liveness_prompts", { settings, count });
}

export function launchTemporaryCli(input: TemporaryCliLaunchInput) {
  return invoke<TemporaryCliLaunchResult>("launch_temporary_cli", { input });
}

export function previewTemporaryCliLaunch(input: TemporaryCliLaunchInput) {
  return invoke<TemporaryCliLaunchPreview>("preview_temporary_cli_launch", { input });
}

export function getCliSessionIndexStatus() {
  return invoke<CliSessionIndexStatus>("get_cli_session_index_status");
}

export function clearCliSessionIndex() {
  return invoke<void>("clear_cli_session_index");
}

export function getCliRuntimeSnapshot() {
  return invoke<CliRuntimeSnapshot>("get_cli_runtime_snapshot");
}

export function getAgentRuntimeSnapshot() {
  return invoke<AgentRuntimeSnapshot>("get_agent_runtime_snapshot");
}

export function activateAgentRuntime(runtimeId: string) {
  return invoke<void>("activate_agent_runtime", { runtimeId });
}

export function getTemporaryCliInstance(instanceId: string) {
  return invoke<TemporaryCliInstance | null>("get_temporary_cli_instance", { instanceId });
}

export function browseWorkspaceDirectories(path?: string) {
  return invoke<WorkspaceDirectoryListing>("browse_workspace_directories", { path });
}

export function forgetWorkspace(path: string) {
  return invoke<Workspace[]>("forget_workspace", { path });
}

export function previewCliConfig(id: string, cliKind: AgentCliKind, apiKeyLocalId: string) {
  return invoke<AgentConfigurationEdit>("preview_cli_config", { id, cliKind, apiKeyLocalId });
}

export function syncAvailableModels(id: string) {
  return invoke<ProviderModelSyncResult>("sync_available_models", { id });
}

export function listProviderApiKeys(id: string) {
  return invoke<ProviderApiKeyOption[]>("list_provider_api_keys", { id });
}

export function listLocalProviderApiKeys(id: string) {
  return invoke<ProviderApiKeyOption[]>("list_local_provider_api_keys", { id });
}

export function addLocalProviderApiKey(id: string, key: string, remark: string) {
  return invoke<Provider>("add_local_provider_api_key", { id, key, remark });
}

export function setLocalProviderApiKeyRemark(id: string, localId: string, remark: string) {
  return invoke<Provider>("set_local_provider_api_key_remark", { id, localId, remark });
}

export function setDefaultLocalProviderApiKey(id: string, localId: string) {
  return invoke<Provider>("set_default_local_provider_api_key", { id, localId });
}

export function removeLocalProviderApiKey(id: string, localId: string) {
  return invoke<Provider>("remove_local_provider_api_key", { id, localId });
}

export function createProviderApiKey(id: string, credentialRevision: number, patch: ProviderApiKeyPatch) {
  return invoke<ProviderApiKeyOption[]>("create_provider_api_key", { id, credentialRevision, patch });
}

export function createProviderApiKeyForInput(input: ProviderInput, patch: ProviderApiKeyPatch) {
  return invoke<ProviderApiKeyOption>("create_provider_api_key_for_input", { input, patch });
}

export function getProviderApiKeyEditorContext(id: string, tokenId: string | null = null) {
  return invoke<ProviderApiKeyEditorContext>("get_provider_api_key_editor_context", { id, tokenId });
}

export function getProviderApiKeyEditorContextForInput(input: ProviderInput) {
  return invoke<ProviderApiKeyEditorContext>("get_provider_api_key_editor_context_for_input", { input });
}

export function updateProviderApiKey(id: string, credentialRevision: number, tokenId: string, patch: ProviderApiKeyPatch) {
  return invoke<ProviderApiKeyOption[]>("update_provider_api_key", { id, credentialRevision, tokenId, patch });
}

export function generateProviderAccessTokenForInput(input: ProviderInput) {
  return invoke<string>("generate_provider_access_token_for_input", { input });
}

export function deleteProviderApiKey(id: string, tokenId: string) {
  return invoke<ProviderApiKeyOption[]>("delete_provider_api_key", { id, tokenId });
}

export function getProviderUsage(id: string, period: string) {
  return invoke<ProviderUsageSummary>("get_provider_usage", { id, period });
}

export function getProviderRequestLogs(id: string, query: ProviderRequestLogsQuery) {
  return invoke<ProviderRequestLogsResult>("get_provider_request_logs", { id, query });
}

export function changeProviderPassword(id: string, originalPassword: string, password: string) {
  return invoke<string>("change_provider_password", { id, originalPassword, password });
}

export function getProviderCheckInRecords(id: string, month: string) {
  return invoke<ProviderCheckInRecordsResult>("get_provider_check_in_records", { id, month });
}

export function probeProviderCapabilities(id: string) {
  return invoke<ProviderCapabilityProbeResult>("probe_provider_capabilities", { id });
}

export function getProviderInviteLink(id: string) {
  return invoke<string>("get_provider_invite_link", { id });
}

export function getSiteAnnouncements() {
  return invoke<SiteAnnouncementsSnapshot>("get_site_announcements");
}

export function markSiteAnnouncementRead(providerId: string, announcementId: string) {
  return invoke<void>("mark_site_announcement_read", { providerId, announcementId });
}

export function refreshProviders(ids: string[]) {
  return invoke<RefreshResult>("refresh_providers", { ids });
}
