import { computed, onMounted, onUnmounted, ref } from "vue";
import { Channel } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { Message } from "@arco-design/web-vue";
import {
  cancelAppUpdate,
  cancelVisibleRelaunch,
  checkAppUpdate,
  clearPendingAppUpdate,
  installAppUpdate,
  type AppUpdateDownloadEvent,
  type AppUpdateInfo,
} from "../api/app";

const STARTUP_UPDATE_CHECK_DELAY_MS = 30 * 1000;
const PERIODIC_UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

type UpdateInstallPhase = "idle" | "downloading" | "verifying" | "installing" | "restarting";

export function useAppUpdater() {
  const checkingForUpdate = ref(false);
  const updateCheckError = ref("");
  const updateDialogVisible = ref(false);
  const availableUpdateCurrentVersion = ref("");
  const availableUpdateVersion = ref("");
  const availableUpdateReleaseNotes = ref("");
  const installingUpdate = ref(false);
  const cancellingUpdate = ref(false);
  const updateDownloadProgress = ref<number | null>(null);
  const updateInstallStatus = ref("");
  const updateInstallError = ref("");
  const updateInstallPhase = ref<UpdateInstallPhase>("idle");
  const updateCanCancel = computed(
    () => installingUpdate.value && updateInstallPhase.value === "downloading",
  );

  let pendingUpdate: AppUpdateInfo | null = null;
  let dismissedVersion: string | null = null;
  let startupCheckTimer: number | null = null;
  let periodicCheckTimer: number | null = null;
  let disposed = false;
  let cancelRequested = false;

  function resetInstallProgress() {
    updateDownloadProgress.value = null;
    updateInstallStatus.value = "";
    updateInstallPhase.value = "idle";
    updateInstallError.value = "";
    cancellingUpdate.value = false;
    cancelRequested = false;
  }

  async function closePendingUpdate() {
    const update = pendingUpdate;
    pendingUpdate = null;
    if (update) {
      await clearPendingAppUpdate().catch(() => {});
    }
  }

  function releaseNotesFromUpdate(update: AppUpdateInfo) {
    const body = update.body?.trim();
    if (body) {
      return body;
    }

    const rawNotes = update.rawJson?.notes;
    if (typeof rawNotes === "string") {
      return rawNotes.trim();
    }

    const rawBody = update.rawJson?.body;
    return typeof rawBody === "string" ? rawBody.trim() : "";
  }

  async function performUpdateCheck(silent: boolean) {
    if (
      disposed ||
      checkingForUpdate.value ||
      installingUpdate.value ||
      updateDialogVisible.value
    ) {
      return;
    }

    checkingForUpdate.value = true;
    updateCheckError.value = "";
    try {
      const update = await checkAppUpdate();
      if (disposed) {
        if (update) {
          await clearPendingAppUpdate().catch(() => {});
        }
        return;
      }
      if (!update) {
        if (!silent) {
          Message.success("当前已是最新版本");
        }
        return;
      }
      if (silent && dismissedVersion === update.version) {
        await clearPendingAppUpdate().catch(() => {});
        return;
      }

      pendingUpdate = update;
      availableUpdateCurrentVersion.value = update.currentVersion;
      availableUpdateVersion.value = update.version;
      availableUpdateReleaseNotes.value = releaseNotesFromUpdate(update);
      resetInstallProgress();
      updateDialogVisible.value = true;
    } catch (error) {
      updateCheckError.value = error instanceof Error ? error.message : String(error);
      if (!silent && !disposed) {
        Message.error(updateCheckError.value);
      }
    } finally {
      checkingForUpdate.value = false;
    }
  }

  function checkForUpdate() {
    return performUpdateCheck(false);
  }

  async function dismissUpdate() {
    if (installingUpdate.value) return;
    dismissedVersion = pendingUpdate?.version ?? dismissedVersion;
    updateDialogVisible.value = false;
    resetInstallProgress();
    await closePendingUpdate();
  }

  async function cancelUpdate() {
    if (!updateCanCancel.value || cancellingUpdate.value) return;

    cancelRequested = true;
    cancellingUpdate.value = true;
    updateInstallStatus.value = "正在取消下载";
    try {
      await cancelAppUpdate();
    } catch (error) {
      cancelRequested = false;
      cancellingUpdate.value = false;
      updateInstallStatus.value =
        updateInstallPhase.value === "downloading" ? "正在下载更新" : updateInstallStatus.value;
      Message.error(error instanceof Error ? error.message : String(error));
    }
  }

  async function installUpdate() {
    const update = pendingUpdate;
    if (!update || installingUpdate.value) return;

    installingUpdate.value = true;
    cancellingUpdate.value = false;
    cancelRequested = false;
    updateInstallPhase.value = "downloading";
    updateInstallStatus.value = "正在准备下载";
    updateInstallError.value = "";
    updateDownloadProgress.value = null;
    let downloadedBytes = 0;
    let contentLength: number | undefined;
    let installed = false;

    function handleDownloadEvent(event: AppUpdateDownloadEvent) {
      if (event.event === "Started") {
        contentLength = event.data.contentLength ?? contentLength;
        updateDownloadProgress.value = contentLength ? 0 : null;
        updateInstallPhase.value = "downloading";
        updateInstallStatus.value = "正在下载更新";
        return;
      }
      if (event.event === "Progress") {
        downloadedBytes += event.data.chunkLength;
        if (contentLength) {
          updateDownloadProgress.value = Math.min(
            99,
            Math.round((downloadedBytes / contentLength) * 100),
          );
        }
        return;
      }
      if (event.event === "Verifying") {
        updateDownloadProgress.value = 100;
        updateInstallPhase.value = "verifying";
        updateInstallStatus.value = "正在校验更新签名";
        cancellingUpdate.value = false;
        return;
      }
      if (event.event === "Installing") {
        updateDownloadProgress.value = 100;
        updateInstallPhase.value = "installing";
        updateInstallStatus.value = "正在安装更新";
        cancellingUpdate.value = false;
        return;
      }

      updateDownloadProgress.value = 100;
      updateInstallStatus.value = "更新安装完成";
    }

    try {
      const onEvent = new Channel<AppUpdateDownloadEvent>();
      onEvent.onmessage = handleDownloadEvent;
      await installAppUpdate(onEvent);
      installed = true;
      pendingUpdate = null;
      updateInstallPhase.value = "restarting";
      updateInstallStatus.value = "正在重启应用";
      await relaunch();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (cancelRequested || message.includes("更新下载已取消")) {
        updateDownloadProgress.value = null;
        updateInstallPhase.value = "idle";
        updateInstallStatus.value = "下载已取消，可重新开始";
      } else if (installed) {
        await cancelVisibleRelaunch().catch(() => {});
        updateDialogVisible.value = false;
        updateInstallError.value = "更新已安装，但应用未能自动重启，请手动重启应用";
        Message.error(updateInstallError.value);
      } else {
        resetInstallProgress();
        updateInstallError.value = `更新安装失败：${message}`;
        Message.error(updateInstallError.value);
      }
    } finally {
      installingUpdate.value = false;
      cancellingUpdate.value = false;
      cancelRequested = false;
    }
  }

  onMounted(() => {
    disposed = false;
    startupCheckTimer = window.setTimeout(() => {
      startupCheckTimer = null;
      void performUpdateCheck(true);
    }, STARTUP_UPDATE_CHECK_DELAY_MS);
    periodicCheckTimer = window.setInterval(() => {
      void performUpdateCheck(true);
    }, PERIODIC_UPDATE_CHECK_INTERVAL_MS);
  });

  onUnmounted(() => {
    disposed = true;
    if (startupCheckTimer !== null) {
      window.clearTimeout(startupCheckTimer);
      startupCheckTimer = null;
    }
    if (periodicCheckTimer !== null) {
      window.clearInterval(periodicCheckTimer);
      periodicCheckTimer = null;
    }
  });

  return {
    checkingForUpdate,
    updateCheckError,
    updateDialogVisible,
    availableUpdateCurrentVersion,
    availableUpdateVersion,
    availableUpdateReleaseNotes,
    installingUpdate,
    cancellingUpdate,
    updateCanCancel,
    updateDownloadProgress,
    updateInstallStatus,
    updateInstallError,
    checkForUpdate,
    dismissUpdate,
    cancelUpdate,
    installUpdate,
  };
}
