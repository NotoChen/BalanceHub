import { invoke } from "@tauri-apps/api/core";
import { withTimeout } from "../utils/promise-timeout";
import type { Provider } from "../stores/provider-types";
import type {
  OAuthVendor,
  OAuthLoginTask,
  OAuthImportFile,
  OAuthImportPreview,
  OAuthImportRequest,
  OAuthAccountDetails,
  OAuthExportFormat,
  OAuthExportResult,
  CpaCredentialFields,
  CpaCredentialFile,
  CpaErrorLog,
  CpaErrorLogContent,
  CpaLogTail,
  CpaLogSettings,
  CpaRequestLog,
  CpaLogClear,
  CpaGatewayConfig,
  CpaApiKeyUsage,
  CpaPluginList,
  CpaPluginRemoval,
  CpaPluginStore,
  CpaStoreInstall,
  CpaPluginConfig,
  CpaModelRuleKind,
  CpaModelRules,
  CpaModelDefinition,
  CpaLoginProvider,
  CpaLoginStart,
  CpaLoginStatus,
  CpaVersionCheck,
  CpaGatewaySettings,
  CpaGatewaySettingsUpdate,
  CpaClientKeys,
  CpaClientKeyAdded,
  CpaAccountQuota,
} from "../stores/oauth-types";

function request<T>(
  name: string,
  args: Record<string, unknown> = {},
  timeout = 25_000,
) {
  return withTimeout(
    invoke<T>(name, args),
    timeout,
    "操作响应超时，请先核对任务或账号状态再重试",
  );
}
export const startOAuthLogin = (
  vendor: OAuthVendor,
  providerId: string | null,
) => request<OAuthLoginTask>("start_oauth_login", { vendor, providerId });
export const listOAuthLogins = () =>
  request<OAuthLoginTask[]>("list_oauth_logins");
export const cancelOAuthLogin = (id: string) =>
  request<void>("cancel_oauth_login", { id });
export const submitOAuthCallback = (id: string, value: string) =>
  request<void>("submit_oauth_callback", { id, value });
export const retryOAuthSave = (id: string) =>
  request<void>("retry_oauth_login_save", { id });
export const openOAuthPage = (id: string) =>
  request<void>("open_oauth_login_page", { id });
export const previewOAuthImport = (
  files: OAuthImportFile[],
  paths: string[],
  vendor: OAuthVendor | null,
) =>
  request<OAuthImportPreview>("preview_oauth_import", { files, paths, vendor });
export const discardOAuthImport = (token: string) =>
  request<void>("discard_oauth_import", { token });
export const applyOAuthImport = (input: OAuthImportRequest) =>
  request<Provider[]>("apply_oauth_import", { request: input });
export const getOAuthAccount = (id: string) =>
  request<OAuthAccountDetails>("get_oauth_account", { id });
export const getOAuthCredential = (id: string) =>
  request<string>("get_oauth_credential", { id });
export const replaceOAuthCredential = (
  id: string,
  content: string,
  revision: number,
) =>
  request<Provider>("replace_oauth_credential", { id, content, revision });
export const setOAuthRenewal = (id: string, enabled: boolean) =>
  request<Provider>("set_oauth_renewal", { id, enabled });
export const updateOAuthAccount = (
  id: string,
  name: string,
  remark: string,
  refreshInterval: number,
) =>
  request<Provider>("update_oauth_account", {
    id,
    name,
    remark,
    refreshInterval,
  });
export const exportOAuthAccount = (
  id: string,
  format: OAuthExportFormat,
  path: string,
  pauseRenewal: boolean,
) =>
  request<OAuthExportResult>("export_oauth_account", {
    id,
    format,
    path,
    pauseRenewal,
  });
export const previewCpaImport = (id: string, names: string[]) =>
  request<OAuthImportPreview>(
    "preview_cpa_oauth_import",
    { id, names },
    120_000,
  );
export const sendOAuthToCpa = (id: string, gatewayId: string) =>
  request<OAuthExportResult>("send_oauth_to_cpa", { id, gatewayId }, 75_000);
export const setCpaDisabled = (
  id: string,
  name: string,
  authIndex: string,
  disabled: boolean,
  revision: number,
) =>
  request<void>(
    "set_cpa_oauth_disabled",
    { id, name, authIndex, disabled, revision },
    75_000,
  );
export const updateCpaFields = (
  id: string,
  name: string,
  authIndex: string,
  fields: CpaCredentialFields,
  revision: number,
) =>
  request<void>(
    "update_cpa_oauth_fields",
    { id, name, authIndex, fields, revision },
    75_000,
  );
export const refreshCpaAccount = (
  id: string,
  name: string,
  authIndex: string,
  revision: number,
) =>
  request<void>(
    "refresh_cpa_oauth",
    { id, name, authIndex, revision },
    120_000,
  );
export const deleteCpaAccount = (
  id: string,
  name: string,
  authIndex: string,
  revision: number,
) =>
  request<void>("delete_cpa_oauth", { id, name, authIndex, revision }, 75_000);
export const viewCpaAccountFile = (id: string, name: string) =>
  request<CpaCredentialFile>("view_cpa_oauth_file", { id, name }, 75_000);
export const resetCpaCooldown = (
  id: string,
  name: string,
  authIndex: string,
  revision: number,
) =>
  request<void>(
    "reset_cpa_oauth_cooldown",
    { id, name, authIndex, revision },
    75_000,
  );
export const refreshAllCpa = (id: string, revision: number) =>
  request<void>("refresh_all_cpa_oauth", { id, revision }, 120_000);
export const listCpaErrorLogs = (id: string) =>
  request<CpaErrorLog[]>("list_cpa_error_logs", { id }, 75_000);
export const readCpaErrorLog = (id: string, name: string) =>
  request<CpaErrorLogContent>("read_cpa_error_log", { id, name }, 75_000);
export const readCpaRequestLogs = (id: string, limit = 200, cursor = "") =>
  request<CpaLogTail>("read_cpa_request_logs", { id, limit, cursor }, 75_000);
export const readCpaApiKeyUsage = (id: string) =>
  request<CpaApiKeyUsage[]>("read_cpa_api_key_usage", { id }, 75_000);
export const readCpaRequestLog = (id: string, requestId: string) =>
  request<CpaRequestLog>("read_cpa_request_log", { id, requestId }, 75_000);
export const clearCpaLogs = (id: string, revision: number) =>
  request<CpaLogClear>("clear_cpa_logs", { id, revision }, 75_000);
/// The gateway's own configuration document, read verbatim. Editing it reaches
/// the values this panel does not model; the tag it returns guards the save.
export const readCpaGatewayConfig = (id: string) =>
  request<CpaGatewayConfig>("read_cpa_gateway_config", { id }, 75_000);
export const updateCpaGatewayConfig = (
  id: string,
  content: string,
  etag: string,
  apiVersion: string,
  revision: number,
) =>
  request<void>(
    "update_cpa_gateway_config",
    { id, content, etag, apiVersion, revision },
    75_000,
  );
export const getCpaLogSettings = (id: string) =>
  request<CpaLogSettings>("get_cpa_log_settings", { id }, 30_000);
export const setCpaLogSettings = (
  id: string,
  enabled: boolean,
  revision: number,
) =>
  request<void>(
    "set_cpa_log_settings",
    { id, enabled, revision },
    75_000,
  );
export const listCpaPlugins = (id: string) =>
  request<CpaPluginList>("list_cpa_plugins", { id }, 75_000);
export const listCpaPluginStore = (id: string) =>
  request<CpaPluginStore>("list_cpa_plugin_store", { id }, 120_000);
export const installCpaPlugin = (
  id: string,
  pluginId: string,
  sourceId: string,
  version: string,
  revision: number,
) =>
  request<CpaStoreInstall>(
    "install_cpa_plugin",
    { id, pluginId, sourceId, version, revision },
    180_000,
  );
export const checkCpaVersion = (id: string) =>
  request<CpaVersionCheck>("check_cpa_version", { id }, 30_000);
export const getCpaPluginConfig = (id: string, pluginId: string) =>
  request<CpaPluginConfig>(
    "get_cpa_plugin_config",
    { id, pluginId },
    75_000,
  );
export const setCpaPluginEnabled = (
  id: string,
  pluginId: string,
  enabled: boolean,
  revision: number,
) =>
  request<void>(
    "set_cpa_plugin_enabled",
    { id, pluginId, enabled, revision },
    75_000,
  );
export const updateCpaPluginConfig = (
  id: string,
  pluginId: string,
  content: string,
  revision: number,
) =>
  request<CpaPluginConfig>(
    "update_cpa_plugin_config",
    { id, pluginId, content, revision },
    75_000,
  );
export const removeCpaPlugin = (
  id: string,
  pluginId: string,
  revision: number,
) =>
  request<CpaPluginRemoval>(
    "remove_cpa_plugin",
    { id, pluginId, revision },
    75_000,
  );
export const getCpaModelRules = (id: string) =>
  request<CpaModelRules>("get_cpa_model_rules", { id }, 75_000);
/// The gateway's own catalog for one channel, read when the rules editor opens.
/// It is per-channel because a model ID means nothing outside the channel that
/// serves it.
export const getCpaModelDefinitions = (id: string, channel: string) =>
  request<CpaModelDefinition[]>(
    "get_cpa_model_definitions",
    { id, channel },
    75_000,
  );
export const updateCpaModelRules = (
  id: string,
  kind: CpaModelRuleKind,
  content: string,
  revision: number,
) =>
  request<CpaModelRules>(
    "update_cpa_model_rules",
    { id, kind, content, revision },
    75_000,
  );
/// The gateway holds the pending login session, so each of these is one short
/// management call rather than a local task.
export const listCpaLoginProviders = (id: string) =>
  request<CpaLoginProvider[]>("list_cpa_login_providers", { id }, 30_000);
export const startCpaLogin = (id: string, provider: string) =>
  request<CpaLoginStart>("start_cpa_login", { id, provider }, 45_000);
export const getCpaLoginStatus = (
  id: string,
  state: string,
  apiVersion: string,
) =>
  request<CpaLoginStatus>(
    "get_cpa_login_status",
    { id, state, apiVersion },
    20_000,
  );
export const submitCpaLoginCallback = (
  id: string,
  provider: string,
  state: string,
  apiVersion: string,
  input: string,
) =>
  request<void>(
    "submit_cpa_login_callback",
    { id, provider, state, apiVersion, input },
    25_000,
  );
export const cancelCpaLogin = (id: string, state: string) =>
  request<void>("cancel_cpa_login", { id, state }, 20_000);
export const openCpaLoginUrl = (id: string, url: string) =>
  request<void>("open_cpa_login_url", { id, url }, 15_000);
export const getCpaGatewaySettings = (id: string) =>
  request<CpaGatewaySettings>("get_cpa_gateway_settings", { id }, 75_000);
export const updateCpaGatewaySettings = (
  id: string,
  update: CpaGatewaySettingsUpdate,
  revision: number,
) =>
  request<void>(
    "update_cpa_gateway_settings",
    { id, update, revision },
    75_000,
  );
export const listCpaClientKeys = (id: string) =>
  request<CpaClientKeys>("list_cpa_client_keys", { id }, 75_000);
export const addCpaClientKey = (
  id: string,
  value: string,
  revision: number,
) =>
  request<CpaClientKeyAdded>(
    "add_cpa_client_key",
    { id, input: { value }, revision },
    75_000,
  );
export const getCpaAccountQuota = (
  id: string,
  authIndex: string,
  pluginId: string,
  apiVersion: string,
) =>
  request<CpaAccountQuota>(
    "get_cpa_account_quota",
    { id, authIndex, pluginId, apiVersion },
    120_000,
  );
export const removeCpaClientKey = (
  id: string,
  value: string,
  revision: number,
) =>
  request<CpaClientKeys>(
    "remove_cpa_client_key",
    { id, value, revision },
    75_000,
  );
