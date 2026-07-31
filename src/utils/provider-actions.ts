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

export function supportsInvitation(provider: Provider) {
  return provider.actions.invitation;
}

export function providerNeedsCheckIn(provider: Provider) {
  return provider.actions.checkIn && !provider.actions.checkedInToday;
}
