import { invoke } from "@tauri-apps/api/core";
import type {
  TemporaryCliAssetListing,
  TemporaryCliProfile,
  TemporaryCliSettingsCapabilities,
  TemporaryCliSettingsRequest,
} from "../stores/temporary-cli-settings-types";

export function getTemporaryCliSettings(request: TemporaryCliSettingsRequest) {
  return invoke<TemporaryCliSettingsCapabilities>(
    "get_temporary_cli_settings",
    { request },
  );
}
export function getTemporaryCliAssets(
  request: TemporaryCliSettingsRequest,
  refresh = false,
) {
  return invoke<TemporaryCliAssetListing>("get_temporary_cli_assets", {
    request,
    refresh,
  });
}
export function listTemporaryCliProfiles() {
  return invoke<TemporaryCliProfile[]>("list_temporary_cli_profiles");
}
export function saveTemporaryCliProfile(profile: TemporaryCliProfile) {
  return invoke<TemporaryCliProfile[]>("save_temporary_cli_profile", {
    profile,
  });
}
export function deleteTemporaryCliProfile(id: string) {
  return invoke<TemporaryCliProfile[]>("delete_temporary_cli_profile", { id });
}
