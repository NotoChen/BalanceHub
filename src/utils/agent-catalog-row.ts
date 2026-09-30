import type { AgentCatalogAsset } from "../stores/agent-catalog-types";
import type { AgentAssetState, AgentCliKind } from "../stores/provider-types";
import { agentCliVisuals, hasAgentCliVisual } from "../agent-cli/visuals";
import { agentAssetStateLabels } from "../composables/useAgentAssetCatalog";
import { formatAgentAssetDiagnostics } from "./agent-environment-diagnostics";

type AgentControlState = AgentAssetState | "missing" | "mixed";
interface AgentControlTarget {
  kind: AgentCliKind;
  state: AgentAssetState | "missing";
  summary: string;
}
export interface AgentCatalogAgentControl {
  kind: AgentCliKind;
  label: string;
  state: AgentControlState;
  stateLabel: string;
  summary: string;
}

/** Full backend identity supplies status; operations come only from the Agent panel IPC. */
export function agentCatalogAgentControls(
  asset: AgentCatalogAsset,
  labels: ReadonlyMap<AgentCliKind, string>,
  agentFilter?: AgentCliKind | null,
): AgentCatalogAgentControl[] {
  const targets: AgentControlTarget[] = [
    ...asset.bindings.filter((binding) => !binding.usage || binding.usage.primaryBindingId === binding.id).map((binding) => {
      const state = binding.usage?.state ?? binding.native.effectiveState;
      return {
        kind: binding.native.agentKind, state,
        summary: [agentAssetStateLabels[state], binding.usage?.detail,
          state === "unknown" ? binding.reason || formatAgentAssetDiagnostics(binding.native.diagnostics)[0] || "尚无完整的状态证据，请查看来源" : null,
        ].filter(Boolean).join(" · "),
      };
    }),
    ...asset.unresolvedTargets.map((target): AgentControlTarget => ({
      kind: target.agentKind,
      state: target.state === "suspended" ? "disabled" : target.state,
      summary: target.state === "suspended" ? "已停用，定义已保留" : target.state === "missing" ? "当前缺失" : "状态待核对",
    })),
  ];
  const groups = new Map<AgentCliKind, AgentControlTarget[]>();
  for (const target of targets) {
    const group = groups.get(target.kind) ?? [];
    group.push(target);
    groups.set(target.kind, group);
  }
  const order = new Map([...labels.keys()].map((kind, index) => [kind, index]));
  const kinds = Object.keys(agentCliVisuals).filter(hasAgentCliVisual)
    .filter((kind) => groups.has(kind))
    .filter((kind) => !agentFilter || kind === agentFilter)
    .sort((left, right) => (order.get(left) ?? labels.size) - (order.get(right) ?? labels.size));
  return kinds.map<AgentCatalogAgentControl>((kind) => {
    const label = labels.get(kind) ?? kind;
    const group = groups.get(kind) ?? [];
    const states = new Set(group.map((target) => target.state));
    const state = states.size === 1 ? group[0].state : "mixed";
    const stateCounts = new Map<string, number>();
    for (const target of group) stateCounts.set(target.summary, (stateCounts.get(target.summary) ?? 0) + 1);
    const summary = group.length > 1
      ? `${group.length} 处配置来源 · ${[...stateCounts].map(([label, count]) => `${count} 处${label}`).join("、")}` : group[0].summary;
    const enabled = group.filter((target) => target.state === "enabled").length;
    const labelText = state === "mixed" ? enabled ? `${enabled} 份已启用 · 多来源` : "多来源 · 状态不同"
      : `${group.length > 1 ? `${group.length} 处` : ""}${state === "missing" ? "缺失" : agentAssetStateLabels[state]}`;
    return { kind, label, state, stateLabel: labelText, summary };
  });
}
