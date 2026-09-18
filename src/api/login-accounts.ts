import { invoke } from "@tauri-apps/api/core";
import type { CredentialKind, LoginAccount, LoginAccountSummary, LoginCookieSummary, LoginPlatform, ProviderCredentialDetails } from "../stores/provider-types";
import type { ProviderBrowserLoginTask } from "./provider-browser-login";
import { withTimeout } from "../utils/promise-timeout";

const call = <T>(command: string, args?: Record<string, unknown>, timeout = 10_000) =>
  withTimeout(invoke<T>(command, args), timeout, "操作超时，请刷新后查看结果");

export const listLoginAccounts = () => call<LoginAccountSummary[]>("list_login_accounts");
export const createLoginAccount = (name: string, platform: LoginPlatform) => call<LoginAccount>("create_login_account", { name, platform });
export const updateLoginAccount = (id: string, name: string, platform: LoginPlatform) => call<void>("update_login_account", { id, name, platform });
export const removeLoginAccount = (id: string, removeEntry: boolean) => call<void>("remove_login_account", { id, removeEntry });
// Startup acknowledgment is bounded/cancelled by the UI flow, including late acknowledgments.
export const openLoginAccount = (id: string, authorizations: boolean) => invoke<ProviderBrowserLoginTask>("open_login_account", { id, authorizations });
export const listLoginCookies = (id: string) => call<LoginCookieSummary[]>("list_login_account_cookies", { id });
export const readLoginCookie = (id: string, cookieId: string) => call<string>("read_login_account_cookie", { id, cookieId });
export const getProviderCredentials = (id: string) => call<ProviderCredentialDetails>("get_provider_credentials", { id });
export const readProviderCredential = (id: string, kind: CredentialKind, revision: number) => call<string>("read_provider_credential", { id, kind, revision });
export const clearProviderCredential = (id: string, kind: CredentialKind, revision: number) => call<void>("clear_provider_credential", { id, kind, revision });
export const validateProviderCredentials = (id: string) => call<ProviderCredentialDetails>("validate_provider_credentials", { id }, 95_000);
