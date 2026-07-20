import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  CliRuntimeSnapshot,
  CliCandidate,
  CodexCliProbeResult,
  LivenessRunResult,
  CodexModelSyncResult,
  LivenessCliKind,
  Provider,
  ProviderApiKeyOption,
  ProviderCapabilityProbeResult,
  ProviderCheckInRecordsResult,
  ProviderCredentialCompletionResult,
  ProviderConnectionTestResult,
  ProviderInput,
  ProviderRequestLogsQuery,
  ProviderRequestLogsResult,
  ProviderSiteProbeResult,
  ProviderUsageSummary,
  TemporaryCliInstance,
} from "../stores/providers";

export type CodexCliProbeInput = Pick<
  AppSettings,
  "livenessCliKind" | "codexCliPath" | "claudeCliPath"
>;

export interface AppData {
  schemaVersion: number;
  providers: Provider[];
  settings: AppSettings;
}

export interface RefreshResult {
  providers: Provider[];
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

export function loadAppData() {
  return invoke<AppData>("load_app_data");
}

export function hostPlatform() {
  return invoke<string>("host_platform");
}

export function userHomeDir() {
  return invoke<string | null>("user_home_dir");
}

export function openCcSwitchDeeplink(url: string) {
  return invoke<void>("open_ccswitch_deeplink", { url });
}

export function saveProvider(input: ProviderInput) {
  return invoke<Provider[]>("save_provider", { input });
}

export function removeProvider(id: string) {
  return invoke<Provider[]>("remove_provider", { id });
}

export function reorderProviders(ids: string[]) {
  return invoke<Provider[]>("reorder_providers", { ids });
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
  return invoke<AppDataTransferResult>("import_app_data", { path });
}

export function completeProviderCredentials(input: ProviderInput) {
  return invoke<ProviderCredentialCompletionResult>("complete_provider_credentials", { input });
}

export function probeProviderSite(input: ProviderInput) {
  return invoke<ProviderSiteProbeResult>("probe_provider_site", { input });
}

export function testProviderConnection(input: ProviderInput) {
  return invoke<ProviderConnectionTestResult>("test_provider_connection", { input });
}

export function probeCodexCli(input?: Partial<CodexCliProbeInput>) {
  return invoke<CodexCliProbeResult>("probe_codex_cli", {
    livenessCliKind: input?.livenessCliKind,
    codexCliPath: input?.codexCliPath,
    claudeCliPath: input?.claudeCliPath,
  });
}

export function previewLivenessPrompts(settings: AppSettings, count = 10) {
  return invoke<string[]>("preview_liveness_prompts", { settings, count });
}

export function testLiveness(id: string, prompt?: string, automatic = false) {
  return invoke<LivenessRunResult>("test_liveness", { id, prompt, automatic });
}

export function launchTemporaryCli(id: string, cliKind: LivenessCliKind, workdir: string) {
  return invoke<TemporaryCliInstance>("launch_temporary_cli", { id, cliKind, workdir });
}

export function getCliRuntimeSnapshot() {
  return invoke<CliRuntimeSnapshot>("get_cli_runtime_snapshot");
}

export function activateTemporaryCli(instanceId: string) {
  return invoke<void>("activate_temporary_cli", { instanceId });
}

export function relaunchTemporaryCli(instanceId: string) {
  return invoke<TemporaryCliInstance>("relaunch_temporary_cli", { instanceId });
}

export function syncCodexModels(id: string) {
  return invoke<CodexModelSyncResult>("sync_codex_models", { id });
}

export function listProviderApiKeys(id: string) {
  return invoke<ProviderApiKeyOption[]>("list_provider_api_keys", { id });
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

export function refreshAllProviders() {
  return invoke<RefreshResult>("refresh_all_providers");
}

export function refreshProviders(ids: string[]) {
  return invoke<RefreshResult>("refresh_providers", { ids });
}

export function acknowledgeLivenessCost() {
  return invoke<AppSettings>("acknowledge_liveness_cost");
}

export function revokeLivenessCost() {
  return invoke<AppSettings>("revoke_liveness_cost");
}

export function checkCliPath(kind: LivenessCliKind, path: string) {
  return invoke<CodexCliProbeResult>("check_cli_path", { kind, path });
}

export function listCliCandidates(kind: LivenessCliKind, path: string) {
  return invoke<CliCandidate[]>("list_cli_candidates", { kind, path });
}
