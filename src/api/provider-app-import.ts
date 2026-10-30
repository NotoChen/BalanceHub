import { invoke } from "@tauri-apps/api/core";
import type {
  ProviderAppImportResult,
  ProviderAppImportTarget,
} from "../stores/provider-types";
import { withTimeout } from "../utils/promise-timeout";

export function openProviderAppImport(
  id: string,
  target: ProviderAppImportTarget,
) {
  return withTimeout(
    invoke<ProviderAppImportResult>("open_provider_app_import", { id, target }),
    30_000,
    "打开应用超时，请检查目标应用是否已打开",
  );
}
