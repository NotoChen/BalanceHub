import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { withTimeout } from "../utils/promise-timeout";

export interface CloudSyncSettings {
  serverUrl: string;
  username: string;
  remoteRoot: string;
  deviceName: string;
  autoSync: boolean;
  hasPassword: boolean;
  hasPassphrase: boolean;
}
export interface CloudSyncSettingsInput {
  serverUrl: string;
  username: string;
  password: string | null;
  passphrase: string | null;
  remoteRoot: string;
  deviceName: string;
  autoSync: boolean;
}
export type CloudSyncPhase =
  | "idle"
  | "checking"
  | "preparing"
  | "review"
  | "downloading"
  | "uploading"
  | "publishing"
  | "applying"
  | "completed"
  | "failed"
  | "cancelled";
export interface CloudSyncChange {
  key: string;
  title: string;
  category: string;
  conflict: boolean;
  upload: boolean;
  download: boolean;
  localDeleted: boolean;
  remoteDeleted: boolean;
}
export interface CloudSyncReview {
  id: string;
  initial: boolean;
  remoteDevice: string;
  remoteUpdatedAt: number | null;
  changes: CloudSyncChange[];
}
export interface CloudSyncStatus {
  revision: number;
  taskId: string;
  phase: CloudSyncPhase;
  message: string;
  running: boolean;
  canCancel: boolean;
  progress: number | null;
  startedAt: number | null;
  finishedAt: number | null;
  lastSyncedAt: number | null;
  retryAt: number | null;
  uploaded: number;
  downloaded: number;
  transferredBytes: number;
  review: CloudSyncReview | null;
  automatic: boolean;
}
export interface CloudSyncSnapshot {
  settings: CloudSyncSettings;
  status: CloudSyncStatus;
  hasRecovery: boolean;
}
export interface CloudSyncResolution {
  key: string;
  side: "local" | "remote";
}
export interface CloudSyncComparison {
  title: string;
  files: {
    path: string;
    localText: string;
    remoteText: string;
    binary: boolean;
  }[];
}

function request<T>(command: string, args?: Record<string, unknown>) {
  return withTimeout(
    invoke<T>(command, args),
    10_000,
    "同步请求响应超时，请查看任务中心或刷新状态",
  );
}
export const cloudSyncApi = {
  state: () => request<CloudSyncSnapshot>("cloud_sync_state"),
  save: (input: CloudSyncSettingsInput) =>
    request<CloudSyncSnapshot>("cloud_sync_save", { input }),
  sync: () => request<CloudSyncSnapshot>("cloud_sync_start"),
  test: () => request<CloudSyncSnapshot>("cloud_sync_test"),
  restore: () => request<CloudSyncSnapshot>("cloud_sync_restore"),
  cancel: (taskId: string) =>
    request<CloudSyncSnapshot>("cloud_sync_cancel", { taskId }),
  confirm: (reviewId: string, resolutions: CloudSyncResolution[]) =>
    request<CloudSyncSnapshot>("cloud_sync_confirm", { reviewId, resolutions }),
  compare: (reviewId: string, key: string) =>
    request<CloudSyncComparison>("cloud_sync_compare", { reviewId, key }),
  listen: (receive: (snapshot: CloudSyncSnapshot) => void) =>
    listen<CloudSyncSnapshot>("cloud-sync-state", (event) =>
      receive(event.payload),
    ),
};
