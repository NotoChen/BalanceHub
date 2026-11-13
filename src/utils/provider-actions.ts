import type { Provider } from "../stores/providers";

export function supportsAccountManagement(provider: Provider) {
  return provider.actions.accountManagement;
}

export function supportsCheckIn(provider: Provider) {
  return provider.actions.checkIn;
}

export function providerCheckedInToday(provider: Provider) {
  return provider.actions.checkedInToday;
}

export function supportsApiKeyManagement(provider: Provider) {
  return provider.actions.apiKeyManagement;
}

/** 站点级用量趋势；能力探测确认不支持时为 false。 */
export function supportsUsage(provider: Provider) {
  return provider.actions.usage;
}

/** 站点请求日志；能力探测确认不支持时为 false。 */
export function supportsRequestLogs(provider: Provider) {
  return provider.actions.requestLogs;
}

export function supportsInvitation(provider: Provider) {
  return provider.actions.invitation;
}

export function providerNeedsCheckIn(provider: Provider) {
  return provider.actions.checkIn && !provider.actions.checkedInToday;
}
