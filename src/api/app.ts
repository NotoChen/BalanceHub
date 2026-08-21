import { invoke, type Channel } from "@tauri-apps/api/core";
import type {
  AppSettings,
  CliConfigPreview,
  CliConfigFile,
  CliRuntimeSnapshot,
  CliSessionDetail,
  CliSessionIndexStatus,
  CliSessionSearchResponse,
  CliEnvironmentProbeResult,
  TerminalEnvironmentProbeResult,
  ProviderModelSyncResult,
  AgentCliKind,
  Provider,
  ProviderProtocolDescriptor,
  ProviderApiKeyOption,
  ProviderCapabilityProbeResult,
  ProviderCheckInRecordsResult,
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
} from "../stores/providers";

export interface AppData {
  revision: number;
  schemaVersion: number;
  providers: Provider[];
  providerProtocols: ProviderProtocolDescriptor[];
  settings: AppSettings;
  workspaces: Workspace[];
  temporaryCliPreferences: TemporaryCliPreference[];
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

export function removeProvider(id: string) {
  return invoke<ProviderRemovalResult>("remove_provider", { id });
}

export function reorderProviders(ids: string[]) {
  return invoke<string[]>("reorder_providers", { ids });
}

export function saveSettings(settings: AppSettings) {
  return invoke<AppSettings>("save_settings", { settings });
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

export function probeTerminals() {
  return invoke<TerminalEnvironmentProbeResult>("probe_terminals");
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

export function searchCliSessions(
  cliKind: AgentCliKind,
  workdir: string,
  query: string,
  limit = 50,
  forceRefresh = false,
) {
  return invoke<CliSessionSearchResponse>("search_cli_sessions", {
    cliKind,
    workdir,
    query,
    limit,
    forceRefresh,
  });
}

export function getCliSessionIndexStatus() {
  return invoke<CliSessionIndexStatus>("get_cli_session_index_status");
}

export function clearCliSessionIndex() {
  return invoke<void>("clear_cli_session_index");
}

export function getCliSessionDetail(
  cliKind: AgentCliKind,
  workdir: string,
  sessionId: string,
) {
  return invoke<CliSessionDetail>("get_cli_session_detail", {
    cliKind,
    workdir,
    sessionId,
  });
}

export function getCliRuntimeSnapshot() {
  return invoke<CliRuntimeSnapshot>("get_cli_runtime_snapshot");
}

export function getTemporaryCliInstances() {
  return invoke<TemporaryCliInstance[]>("get_temporary_cli_instances");
}

export function getTemporaryCliInstance(instanceId: string) {
  return invoke<TemporaryCliInstance | null>("get_temporary_cli_instance", { instanceId });
}

export function activateTemporaryCli(instanceId: string) {
  return invoke<void>("activate_temporary_cli", { instanceId });
}

export function browseWorkspaceDirectories(path?: string) {
  return invoke<WorkspaceDirectoryListing>("browse_workspace_directories", { path });
}

export function forgetWorkspace(path: string) {
  return invoke<Workspace[]>("forget_workspace", { path });
}

export function previewCliConfig(id: string, cliKind: AgentCliKind) {
  return invoke<CliConfigPreview>("preview_cli_config", { id, cliKind });
}

export function switchCliConfig(
  id: string,
  cliKind: AgentCliKind,
  revision: string,
  files: CliConfigFile[],
) {
  return invoke<CliRuntimeSnapshot>("switch_cli_config", { id, cliKind, revision, files });
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

export function setPrimaryLocalProviderApiKey(id: string, localId: string) {
  return invoke<Provider>("set_primary_local_provider_api_key", { id, localId });
}

export function removeLocalProviderApiKey(id: string, localId: string) {
  return invoke<Provider>("remove_local_provider_api_key", { id, localId });
}

export function createProviderApiKey(id: string, name: string) {
  return invoke<ProviderApiKeyOption[]>("create_provider_api_key", { id, name });
}

export function createProviderApiKeyForInput(input: ProviderInput, name: string) {
  return invoke<ProviderApiKeyOption>("create_provider_api_key_for_input", { input, name });
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
