import { computed, onMounted, onScopeDispose } from "vue";
import { listen, type UnlistenFn } from "../api/transport.ts";
import { Message } from "@arco-design/web-vue";
import { useOAuthAccountStore } from "../stores/oauth-accounts";
import { useProviderStore } from "../stores/providers";
import {
  cancelOAuthLogin,
  listOAuthLogins,
  retryOAuthSave,
} from "../api/oauth-accounts";
import { withTimeout } from "../utils/promise-timeout";
import { oauthVendorLabel } from "../utils/oauth-display";
import type { OAuthLoginTask } from "../stores/oauth-types";
import type { BackgroundTask } from "./useBackgroundTaskCenter";

export function useOAuthTasks() {
  const store = useOAuthAccountStore();
  const providers = useProviderStore();
  const completed = new Set<string>();
  /// 已提示过刷新失败的授权任务。任务保留在 completed 之外，下一次轮询仍会
  /// 安静重试；但不能每 15 秒把同一条失败再弹一次。
  const reportedRefreshFailures = new Set<string>();
  let disposed = false;
  let unlisten: UnlistenFn | undefined;
  let recovering = false;
  let connecting = false;
  let timer: ReturnType<typeof setInterval> | undefined;
  const connectedAt = Date.now();

  async function accept(task: OAuthLoginTask) {
    if (disposed) return;
    store.accept(task);
    if (
      task.phase !== "completed" ||
      !task.providerId ||
      completed.has(task.id)
    )
      return;
    completed.add(task.id);
    try {
      // 授权只影响这一张卡片；补拉整份配置会把其他站点的视图也重新解析。
      const provider = await withTimeout(
        providers.reloadProvider(task.providerId),
        15_000,
        "账号已保存，面板刷新超时",
      );
      if (provider && !provider.automation.lastSyncedAt)
        await providers.refreshByIds([provider.identity.id]);
      reportedRefreshFailures.delete(task.id);
    } catch (error) {
      completed.delete(task.id);
      // 列表刷新失败时保留重试：下一次轮询会把新账号补进来。失败说明只提示
      // 一次，重试成功则不再提；重复弹同一条错误没有新的信息。
      if (!disposed && !reportedRefreshFailures.has(task.id)) {
        reportedRefreshFailures.add(task.id);
        Message.error(String(error));
      }
    }
  }
  async function recover() {
    if (recovering || disposed) return;
    recovering = true;
    const requestedAt = Date.now();
    try {
      const listed = await listOAuthLogins();
      if (disposed) return;
      const present = new Set(listed.map((task) => task.id));
      for (const task of listed) void accept(task);
      // 授权任务只存在于后端内存里：应用重启后，未结束的任务不会再有终态
      // 事件，列表里也不会再出现。不收口的话任务中心会一直停在等待中，并
      // 每 15 秒轮询一份永远不含它的列表。只处理创建时间早于本次请求的
      // 任务，避免把列表请求期间刚发起的授权误判为已消失。
      for (const task of Object.values(store.tasks)) {
        if (present.has(task.id)) continue;
        if (!["starting", "waiting", "exchanging"].includes(task.phase)) continue;
        if (task.createdAt * 1000 + 1_000 >= requestedAt) continue;
        store.accept({
          ...task,
          revision: task.revision + 1,
          updatedAt: Math.floor(Date.now() / 1000),
          phase: "failed",
          message: "授权任务已从后台列表消失（应用可能已重启），请重新发起授权",
          canCancel: false,
          canRetrySave: false,
        });
      }
      if (!disposed && unlisten) store.eventError = "";
    } catch {
      if (!disposed) store.eventError = "授权状态暂时未连接，后台任务仍在继续";
    } finally {
      recovering = false;
    }
  }
  async function connect() {
    if (connecting || unlisten || disposed) return;
    connecting = true;
    let expired = false;
    try {
      const stop = await withTimeout(
        listen<OAuthLoginTask>("oauth-login-task", ({ payload }) => {
          if (!disposed && !expired) void accept(payload);
        }).then((stop) => {
          if (disposed || expired) stop();
          return stop;
        }),
        10_000,
        "授权状态连接超时",
      );
      if (disposed) stop();
      else {
        unlisten = stop;
        store.eventError = "";
      }
    } catch {
      expired = true;
      if (!disposed) store.eventError = "授权状态暂时未连接，正在重连";
    } finally {
      connecting = false;
    }
    await recover();
  }
  async function control(operation: () => Promise<void>) {
    try {
      await operation();
      await recover();
    } catch (error) {
      Message.error(String(error));
    }
  }
  onMounted(() => {
    void connect();
    timer = setInterval(() => {
      if (!unlisten) void connect();
      if (
        store.visible ||
        Object.values(store.tasks).some((task) =>
          ["starting", "waiting", "exchanging"].includes(task.phase),
        )
      )
        void recover();
    }, 15_000);
  });
  onScopeDispose(() => {
    disposed = true;
    clearInterval(timer);
    unlisten?.();
  });

  return computed<BackgroundTask[]>(() => {
    const tasks = Object.values(store.tasks).map((task): BackgroundTask => {
      const status =
        task.phase === "completed"
          ? "success"
          : task.phase === "failed"
            ? "failed"
            : task.phase === "canceled"
              ? "cancelled"
              : task.phase === "waiting"
                ? "waiting"
                : "running";
      return {
        id: `oauth:${task.id}`,
        kind: "providerLogin",
        title: `${oauthVendorLabel(task.vendor)} 账号授权`,
        detail: task.message,
        status,
        progress: null,
        startedAt: task.createdAt * 1000,
        finishedAt: ["completed", "failed", "canceled"].includes(task.phase)
          ? task.updatedAt * 1000
          : undefined,
        source: "manual",
        error: task.phase === "failed" ? task.message : undefined,
        actions: [
          {
            label: task.phase === "completed" ? "查看账号" : "查看授权",
            run: () =>
              task.phase === "completed" && task.providerId
                ? store.openAccount(task.providerId)
                : store.openTask(task.id),
          },
          ...(task.canCancel
            ? [
                {
                  label: "取消",
                  run: () => {
                    void control(() => cancelOAuthLogin(task.id));
                  },
                },
              ]
            : []),
          ...(task.canRetrySave
            ? [
                {
                  label: "重试保存",
                  run: () => {
                    void control(() => retryOAuthSave(task.id));
                  },
                },
              ]
            : []),
        ],
      };
    });
    if (store.eventError)
      tasks.push({
        id: "oauth:connection",
        kind: "providerLogin",
        title: "授权状态连接",
        detail: store.eventError,
        status: "unconfirmed",
        progress: null,
        startedAt: connectedAt,
        source: "automatic",
        actions: [
          {
            label: "重新连接",
            run: () => {
              void connect();
            },
          },
        ],
      });
    return tasks;
  });
}
