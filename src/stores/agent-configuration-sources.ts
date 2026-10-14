import { defineStore } from "pinia";
import { onScopeDispose } from "vue";
import { listAgentConfigurationSources } from "../api/agent-configuration";
import { createAgentPublicationCache, type AgentPublicationLoader } from "../utils/agent-publication-cache";
import { agentConfigurationErrorMessage } from "../utils/agent-configuration-display";
import type { AgentConfigurationSnapshot } from "./agent-configuration-types";
import type { AgentCliKind, AgentRuntimeScope } from "./provider-types";

export type AgentConfigurationPublisher = AgentPublicationLoader<AgentConfigurationSnapshot>;

export const useAgentConfigurationSourcesStore = defineStore("agent-configuration-sources", () => {
  const caches = new Map<string, ReturnType<typeof makeCache>>();
  function makeCache(runtimeScope: AgentRuntimeScope) { return createAgentPublicationCache({
    load: (agentKind, workspace) => listAgentConfigurationSources({ agentKind, workspace, runtimeScope }),
    timeoutMs: 30_000,
    error: (failure) => agentConfigurationErrorMessage(failure, "无法读取配置来源，请重试"),
  }); }
  function forRuntime(scope: AgentRuntimeScope = { kind: "native" }) {
    const key = JSON.stringify(scope);
    let value = caches.get(key);
    if (!value) { value = makeCache(scope); caches.set(key, value); }
    return value;
  }
  const cache = forRuntime();
  function invalidateAllRuntimes(kind: AgentCliKind) {
    for (const entry of caches.values()) entry.invalidate(kind);
  }
  onScopeDispose(() => { for (const entry of caches.values()) entry.dispose(); caches.clear(); });
  return { ...cache, forRuntime, invalidateAllRuntimes };
});
