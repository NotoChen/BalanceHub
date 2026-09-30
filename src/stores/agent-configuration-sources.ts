import { defineStore } from "pinia";
import { onScopeDispose } from "vue";
import { listAgentConfigurationSources } from "../api/agent-configuration";
import { createAgentPublicationCache, type AgentPublicationLoader } from "../utils/agent-publication-cache";
import { agentConfigurationErrorMessage } from "../utils/agent-configuration-display";
import type { AgentConfigurationSnapshot } from "./agent-configuration-types";

export type AgentConfigurationPublisher = AgentPublicationLoader<AgentConfigurationSnapshot>;

export const useAgentConfigurationSourcesStore = defineStore("agent-configuration-sources", () => {
  const cache = createAgentPublicationCache({
    load: (agentKind, workspace) => listAgentConfigurationSources({ agentKind, workspace }),
    timeoutMs: 30_000,
    error: (failure) => agentConfigurationErrorMessage(failure, "无法读取配置来源，请重试"),
  });
  onScopeDispose(cache.dispose);
  return cache;
});
