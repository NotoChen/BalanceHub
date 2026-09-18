import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { withTimeout } from "../utils/promise-timeout";

export interface CheckInTask {
  runId: string;
  providerId: string;
  providerName: string;
  batchId: string | null;
  source: "manual" | "batch" | "automatic";
  phase: "queued" | "checking" | "opening" | "verifying" | "waitingHuman" | "waitingBrowser"
    | "requesting" | "verifyingResult" | "saving" | "completed" | "failed" | "cancelled" | "unconfirmed";
  message: string;
  revision: number;
  finished: boolean;
  canResume: boolean;
  canCancel: boolean;
  startedAt: number;
  finishedAt: number | null;
}

export interface CheckInBatch {
  batchId: string;
  tasks: CheckInTask[];
  skipped: number;
}

export function checkInProvider(id: string) {
  return withTimeout(invoke<CheckInTask>("check_in_provider", { id }), 8_000, "提交签到任务超时，可在后台任务中查看");
}

export function checkInAllProviders() {
  return withTimeout(invoke<CheckInBatch>("check_in_all_providers"), 8_000, "提交批量签到超时，可在后台任务中查看");
}

export function listCheckInTasks() {
  return withTimeout(invoke<CheckInTask[]>("list_check_in_tasks"), 8_000, "读取签到任务超时");
}

export function resumeCheckInTask(runId: string) {
  return withTimeout(invoke<CheckInTask>("resume_check_in_task", { runId }), 8_000, "继续签到超时");
}

export function cancelCheckInTask(runId: string) {
  return withTimeout(invoke<void>("cancel_check_in_task", { runId }), 5_000, "取消请求超时，请查看后台任务状态");
}

export function listenCheckInTasks(receive: (task: CheckInTask) => void) {
  return listen<CheckInTask>("balancehub://check-in-task", (event) => receive(event.payload));
}
