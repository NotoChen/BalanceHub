import { defineStore } from "pinia";
import { shallowRef, ref } from "vue";
import {
  applyAgentMaintenance,
  getAgentMaintenanceOperation,
  listAgentMaintenanceOperations,
  cancelAgentMaintenanceOperation,
  discardAgentMaintenancePlan,
} from "../api/agent-maintenance";
import { useAgentOperationTracking } from "../composables/useAgentOperationTracking";
import { withTimeout } from "../utils/promise-timeout";
import { agentEnvironmentErrorMessage } from "./agent-environment";
import { useAgentCatalogStore } from "./agent-catalog";
import { useAgentOverviewStore } from "./agent-overview";
import { useAgentConfigurationSourcesStore } from "./agent-configuration-sources";
import type {
  AgentMaintenancePlan,
  AgentMaintenanceOperation,
} from "./agent-maintenance-types";

export const useAgentMaintenanceStore = defineStore("agent-maintenance", () => {
  const preview = shallowRef<AgentMaintenancePlan | null>(null);
  const submissions = ref<
    Record<
      string,
      { title: string; startedAt: number; pending: boolean; error: string }
    >
  >({});
  const historyRevision = ref(0);
  const tracking = useAgentOperationTracking<AgentMaintenanceOperation>({
    get: getAgentMaintenanceOperation,
    list: listAgentMaintenanceOperations,
    cancel: cancelAgentMaintenanceOperation,
    identity: (operation) => `${operation.kind}:${operation.planToken}`,
    isSettled: (operation) => operation.phase === "completed",
    timeoutMs: 11 * 60_000,
    accepted(operation) {
      delete submissions.value[operation.planToken];
    },
    completed(operation) {
      if (operation.refresh.catalog) {
        const catalog = useAgentCatalogStore();
        for (const snapshot of Object.values(catalog.catalogs))
          catalog.invalidate(snapshot.inventory.workspace ?? undefined);
      }
      if (!operation.refresh.agents.length) return;
      historyRevision.value += 1;
      const overview = useAgentOverviewStore();
      const sources = useAgentConfigurationSourcesStore();
      for (const kind of operation.refresh.agents) {
        overview.invalidate(kind);
        sources.invalidateAllRuntimes(kind);
      }
    },
  });
  function discard(plan: AgentMaintenancePlan) {
    void withTimeout(
      discardAgentMaintenancePlan(plan.token),
      10_000,
      "释放预览超时",
    ).catch(() => {});
  }
  function present(plan: AgentMaintenancePlan) {
    if (preview.value?.token !== plan.token) closePreview();
    preview.value = plan;
  }
  function closePreview() {
    const plan = preview.value;
    preview.value = null;
    if (plan) discard(plan);
  }
  async function presentWhenReady(
    pending: Promise<AgentMaintenancePlan>,
    active: () => boolean,
    timeoutMs: number,
    message: string,
  ) {
    let waiting = true;
    void pending.then(
      (plan) => {
        if (!waiting) discard(plan);
      },
      () => {},
    );
    try {
      const plan = await withTimeout(pending, timeoutMs, message);
      if (active()) present(plan);
      else discard(plan);
    } finally {
      waiting = false;
    }
  }
  async function submit() {
    const plan = preview.value;
    if (!plan || submissions.value[plan.token]?.pending) return;
    preview.value = null;
    submissions.value[plan.token] = {
      title: plan.title,
      startedAt: Date.now(),
      pending: true,
      error: "",
    };
    try {
      tracking.track(
        await withTimeout(
          applyAgentMaintenance(plan),
          15_000,
          "提交任务超时，请在任务中心刷新状态；不会自动重复提交",
        ),
      );
    } catch (error) {
      if (submissions.value[plan.token])
        submissions.value[plan.token].error =
          agentEnvironmentErrorMessage(error);
      void tracking.recover();
    } finally {
      if (submissions.value[plan.token])
        submissions.value[plan.token].pending = false;
    }
  }
  return {
    preview,
    submissions,
    historyRevision,
    present,
    presentWhenReady,
    discard,
    closePreview,
    submit,
    ...tracking,
  };
});
