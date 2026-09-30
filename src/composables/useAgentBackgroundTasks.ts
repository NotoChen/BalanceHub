import { computed } from "vue";
import { useAgentCatalogStore } from "../stores/agent-catalog";
import { useAgentLifecycleStore } from "../stores/agent-lifecycle";
import { useAgentEnvironmentStore } from "../stores/agent-environment";
import { useAgentSessionResumeStore } from "../stores/agent-session-resume";
import { useAgentConfigurationStore } from "../stores/agent-configuration";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import { useAgentWorkspaceStore } from "../stores/agent-workspace";
import { agentAssetOperationOutcomeLabels, agentAssetOperationPhaseLabels } from "./useAgentAssetConsole";
import type { BackgroundTask, BackgroundTaskStatus } from "./useBackgroundTaskCenter";
import type { AgentAssetOperationOutcome } from "../stores/provider-types";
import { agentCatalogActionLabels, agentCatalogOperationDetail, agentCatalogRelationActionLabels } from "../utils/agent-catalog-display";

const outcomeText = (outcome: AgentAssetOperationOutcome | null) => outcome ? agentAssetOperationOutcomeLabels[outcome] : "结果待确认";
const outcomeStatuses: Record<AgentAssetOperationOutcome, BackgroundTaskStatus> = {
  appliedVerified: "success", appliedUnverified: "unconfirmed", outcomeUnknown: "unconfirmed",
  canceledBeforeCommit: "cancelled", unchangedConflict: "failed", unchangedFailure: "failed",
};
const outcomeStatus = (outcome: AgentAssetOperationOutcome | null): BackgroundTaskStatus => outcome ? outcomeStatuses[outcome] : "unconfirmed";

export function useAgentBackgroundTasks() {
  const catalog = useAgentCatalogStore();
  const lifecycle = useAgentLifecycleStore();
  const native = useAgentEnvironmentStore();
  const sessions = useAgentSessionResumeStore();
  const configuration = useAgentConfigurationStore();
  const cli = useCliRuntimeStore();
  const navigation = useAgentWorkspaceStore();
  return computed<BackgroundTask[]>(() => {
    const tasks: BackgroundTask[] = [];
    for (const operation of Object.values(catalog.operations)) {
      const completed = operation.phase === "completed";
      const verified = operation.targets.filter((target) => target.outcome === "appliedVerified").length;
      const finished = operation.targets.filter((target) => target.phase === "completed").length;
      const name = catalog.operationNames[operation.id] || Object.values(catalog.catalogs).flatMap((snapshot) => snapshot.assets).find((asset) => asset.id === operation.assetId)?.name || "资源";
      tasks.push({ id: `agent-catalog-${operation.id}`, kind: "sync", title: `${name} · ${agentCatalogActionLabels[operation.action]}`,
        detail: [catalog.errors[operation.id], agentCatalogOperationDetail(operation)].filter(Boolean).join("；"),
        status: completed ? (operation.targets.length > 0 && verified === operation.targets.length && (!operation.definitionChange || operation.definitionChange.state === "saved") ? "success" : "failed") : "running",
        progress: operation.targets.length ? finished / operation.targets.length : null, startedAt: Date.parse(operation.createdAt),
        finishedAt: completed ? Date.parse(operation.updatedAt) : undefined, source: "manual",
        actions: [{ label: "查看详情", run: () => navigation.openOperation("catalog", operation.id) }] });
    }
    for (const task of Object.values(catalog.relationTasks)) {
      tasks.push({ id: task.id, kind: "sync", title: `${task.name} · ${agentCatalogRelationActionLabels[task.action]}`,
        detail: task.message, status: task.state === "running" ? "running" : task.state === "completed" ? "success" : "failed",
        progress: null, startedAt: task.startedAt, finishedAt: task.finishedAt, source: "manual",
        actions: task.state === "unknown" || task.state === "failed"
          ? [{ label: "重新检查", run: () => { void catalog.refreshRelation(task.id); } }] : [] });
    }
    for (const operation of Object.values(lifecycle.operations)) {
      const completed = operation.phase === "completed";
      tasks.push({ id: `agent-lifecycle-${operation.id}`, kind: "update", title: `${operation.agentKind} · 升级`,
        detail: lifecycle.errors[operation.id] || operation.message || (completed ? `${outcomeText(operation.outcome)}${operation.observedVersion ? ` · ${operation.observedVersion}` : ""}` : agentAssetOperationPhaseLabels[operation.phase]),
        status: lifecycle.errors[operation.id] ? "unconfirmed" : completed ? outcomeStatus(operation.outcome) : "running", progress: null,
        startedAt: Date.parse(operation.createdAt), finishedAt: completed ? Date.parse(operation.updatedAt) : undefined, source: "manual",
        actions: [{ label: "查看详情", run: () => navigation.openOperation("lifecycle", operation.id) }] });
    }
    for (const operation of Object.values(native.operations)) {
      if (catalog.nativeOperationIds.has(operation.id)) continue;
      const completed = operation.phase === "completed";
      tasks.push({ id: `agent-native-${operation.id}`, kind: "sync", title: "原生资产操作",
        detail: completed ? outcomeText(operation.outcome) : agentAssetOperationPhaseLabels[operation.phase],
        status: completed ? outcomeStatus(operation.outcome) : "running", progress: null,
        startedAt: Date.parse(operation.createdAt), finishedAt: completed ? Date.parse(operation.updatedAt) : undefined, source: "manual",
        actions: [{ label: "查看详情", run: () => navigation.openOperation("native", operation.id) }] });
    }
    for (const [prefix, store, title] of [["catalog", catalog, "提交资产操作"], ["lifecycle", lifecycle, "提交升级任务"]] as const) {
      for (const [id, startedAt] of Object.entries(store.startTimes)) {
        if (!store.starting[id] && !store.startErrors[id]) continue;
        tasks.push({ id: `agent-${prefix}-start-${id}`, kind: prefix === "lifecycle" ? "update" : "sync", title,
          detail: store.startErrors[id] || "正在确认后台任务", status: store.starting[id] ? "running" : "failed", progress: null,
          startedAt, finishedAt: store.starting[id] ? undefined : startedAt, source: "manual", error: store.startErrors[id],
          actions: store.startErrors[id] ? [{ label: "刷新状态", disabled: store.recovering, run: () => { void store.recover(); } }] : [] });
      }
    }
    for (const operation of Object.values(sessions.operations)) {
      const settled = operation.state !== "queued" && operation.state !== "running";
      const failure = sessions.errors[operation.id];
      const label = cli.cliRuntime.agents.find((agent) => agent.kind === operation.cliKind)?.label || operation.cliKind || "Agent";
      tasks.push({ id: `agent-session-resume-${operation.id}`, kind: "cliLaunch", title: `${label} · 继续会话`,
        detail: [failure || operation.message, operation.result?.workspaceError].filter(Boolean).join("；"),
        status: failure || operation.state === "uncertain" ? "unconfirmed" : operation.state === "cancelled" ? "cancelled" : settled ? operation.state === "succeeded" ? "success" : "failed" : "running",
        progress: null, startedAt: Date.parse(operation.createdAt), finishedAt: settled || failure ? Date.parse(operation.updatedAt) : undefined,
        source: "manual", error: failure,
        actions: [
          ...(operation.canCancel ? [{ label: "取消", disabled: Boolean(sessions.canceling[operation.id]), run: () => { void sessions.cancel(operation.id); } }] : []),
          ...(!settled || failure || operation.state === "uncertain" ? [{ label: "刷新状态", disabled: sessions.recovering, run: () => { void sessions.recover(); } }] : []),
        ] });
    }
    for (const [sessionRef, startedAt] of Object.entries(sessions.startTimes)) {
      if ((!sessions.starting[sessionRef] && !sessions.startErrors[sessionRef]) || sessions.operationFor(sessionRef)?.requestId === sessions.requestIds[sessionRef]) continue;
      tasks.push({ id: `agent-session-resume-start-${sessionRef}`, kind: "cliLaunch", title: "提交会话继续",
        detail: sessions.startErrors[sessionRef] || "正在确认后台任务", status: sessions.starting[sessionRef] ? "running" : "failed",
        progress: null, startedAt, finishedAt: sessions.starting[sessionRef] ? undefined : startedAt, source: "manual", error: sessions.startErrors[sessionRef],
        actions: [{ label: "刷新状态", disabled: sessions.recovering, run: () => { void sessions.recover(); } }] });
    }
    for (const operation of Object.values(configuration.operations)) {
      const completed = operation.phase === "completed";
      const failure = configuration.errors[operation.id];
      const label = configuration.contexts[operation.editId]?.label || cli.cliRuntime.agents.find((agent) => agent.kind === operation.agentKind)?.label || "Agent 配置";
      const result = operation.outcome === "appliedVerified" ? "已写入并校验文件" : operation.outcome === "appliedUnverified" ? "已写入，文件校验待确认" : outcomeText(operation.outcome);
      const files = operation.files.map((file, index) => "文件 " + (index + 1) + "：" + (file.state === "applied" ? "已写入" : file.state === "unchanged" ? "未修改" : "结果未知") + (file.message ? " · " + file.message : ""));
      tasks.push({ id: "agent-configuration-" + operation.id, kind: "sync", title: label + " · 保存配置",
        detail: [failure, completed ? result : agentAssetOperationPhaseLabels[operation.phase], operation.message, ...files, ...operation.reloadHints].filter(Boolean).join("；"),
        status: failure ? "unconfirmed" : completed ? outcomeStatus(operation.outcome) : "running",
        progress: null, startedAt: Date.parse(operation.createdAt), finishedAt: completed ? Date.parse(operation.updatedAt) : undefined, source: "manual", error: failure,
        actions: [
          ...(operation.canCancel ? [{ label: "取消", disabled: Boolean(configuration.canceling[operation.id]), run: () => { void configuration.cancel(operation.id); } }] : []),
          ...(!completed ? [{ label: "刷新状态", disabled: configuration.recovering, run: () => { void configuration.recover(); } }] : []),
        ],
      });
    }
    for (const [editId, startedAt] of Object.entries(configuration.startTimes)) {
      if ((!configuration.starting[editId] && !configuration.startErrors[editId]) || configuration.operationFor(editId)) continue;
      tasks.push({ id: "agent-configuration-start-" + editId, kind: "sync", title: (configuration.contexts[editId]?.label || "配置") + " · 提交保存",
        detail: configuration.startErrors[editId] || "正在确认后台任务", status: configuration.starting[editId] ? "running" : configuration.declined[editId] ? "failed" : "unconfirmed",
        progress: null, startedAt, source: "manual", error: configuration.startErrors[editId],
        actions: [{ label: "刷新状态", disabled: configuration.recovering, run: () => { void configuration.recover(); } }],
      });
    }
    return tasks;
  });
}
