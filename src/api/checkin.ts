import { invoke } from "@tauri-apps/api/core";

export interface ProviderCheckInResult {
  ok: boolean;
  message: string;
  lastCheckedInAt?: string;
  lastCheckInUser?: string;
}

export function checkInProvider(id: string) {
  return invoke<ProviderCheckInResult>("check_in_provider", { id });
}
