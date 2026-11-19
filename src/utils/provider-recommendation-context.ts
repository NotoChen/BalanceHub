import type { AgentCliKind } from "../stores/provider-types";

/// 从中转站推荐弹窗的角度看，一个 Agent 的"当前状态"只有三件事：它现在绑在
/// 哪个站、用哪个模型、叫什么名字。传这三件事进去，推荐就能把"当前使用"
/// 从一堆候选中标出来，并把默认目标设成这个 Agent。
export interface RecommendationAgentContext {
  kind: AgentCliKind;
  label: string;
  providerId: string;
  providerLabel: string;
  model: string;
}
