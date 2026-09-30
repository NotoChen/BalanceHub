import { computed, reactive, ref, watch } from "vue";
import type { AgentAssetCategory, AgentCliDescriptor, AgentCliKind } from "../stores/provider-types";
import type { AgentCatalogDefinition, AgentCatalogHookVariantInput, AgentCatalogMcpInput, AgentCatalogSaveRequest } from "../stores/agent-catalog-types";

interface AgentDefinitionEditorProps {
  visible: boolean;
  definition: AgentCatalogDefinition | null;
  initialDraft?: AgentCatalogSaveRequest | null;
  editingAssetId: string | null;
  category: AgentAssetCategory;
  loading: boolean;
  saving: boolean;
  agents?: readonly AgentCliDescriptor[];
  preferredAgentKind?: AgentCliKind | null;
}

export interface AgentCatalogDefinitionFormProps extends AgentDefinitionEditorProps {
  error: string;
  agents: AgentCliDescriptor[];
  preferredAgentKind: AgentCliKind | null;
  embedded?: boolean;
}

export function useAgentDefinitionEditor(props: Readonly<AgentDefinitionEditorProps>, onSave: (request: AgentCatalogSaveRequest, intent: "save" | "apply") => void) {
  const category = computed(() => props.definition?.category ?? props.category);
  const draft = reactive({ name: "", category, mcpJson: "{}", skillMarkdown: "",
    hookVariants: [] as AgentCatalogHookVariantInput[] });
  const originalMcpJson = computed(() => props.definition?.mcp ? JSON.stringify(props.definition.mcp, null, 2) : "{}");
  const validationError = ref("");
  const editing = computed(() => Boolean(props.editingAssetId));
  const activeHookAgent = ref<AgentCliKind | null>(null);
  const activeHookVariant = computed(() => draft.hookVariants.find((variant) => variant.agentKind === activeHookAgent.value) ?? null);
  const availableHookAgents = computed(() => (props.agents ?? []).filter((agent) => !draft.hookVariants.some((variant) => variant.agentKind === agent.kind)));
  function initializeHookVariant() {
    if (draft.category !== "hook" || draft.hookVariants.length) return;
    const kind = props.preferredAgentKind ?? props.agents?.[0]?.kind;
    if (kind) {
      draft.hookVariants.push({ agentKind: kind, event: "", groupJson: "" });
      activeHookAgent.value = kind;
    }
  }
  watch(() => [props.visible, props.definition, props.category, props.initialDraft] as const, () => {
    if (!props.visible) return;
    const definition = props.definition;
    const initial = props.initialDraft;
    draft.name = initial?.name ?? definition?.name ?? "";
    draft.mcpJson = initial?.mcp ? JSON.stringify(initial.mcp, null, 2) : originalMcpJson.value;
    draft.skillMarkdown = initial?.skillMarkdown ?? definition?.skillMarkdown ?? "";
    draft.hookVariants = (initial?.hook?.variants ?? definition?.hook?.variants ?? []).map((variant) => ({ ...variant }));
    activeHookAgent.value = draft.hookVariants.find((variant) => variant.agentKind === props.preferredAgentKind)?.agentKind
      ?? draft.hookVariants[0]?.agentKind ?? null;
    initializeHookVariant();
    validationError.value = "";
  }, { immediate: true });
  watch(() => props.preferredAgentKind, (kind) => {
    if (kind && draft.hookVariants.some((variant) => variant.agentKind === kind)) activeHookAgent.value = kind;
  });

  function addHookVariant(value: unknown) {
    if (props.loading || props.saving) return;
    const agent = availableHookAgents.value.find((item) => item.kind === value);
    if (!agent) return;
    // Every Agent receives an explicit native variant; events and payloads are never converted.
    draft.hookVariants.push({ agentKind: agent.kind, event: "", groupJson: "" });
    activeHookAgent.value = agent.kind;
    validationError.value = "";
  }
  function removeHookVariant(kind: AgentCliKind) {
    if (props.loading || props.saving) return;
    draft.hookVariants = draft.hookVariants.filter((variant) => variant.agentKind !== kind);
    if (activeHookAgent.value === kind) activeHookAgent.value = draft.hookVariants[0]?.agentKind ?? null;
  }

  function parseMcpJson(): AgentCatalogMcpInput {
    let value: unknown;
    try { value = JSON.parse(draft.mcpJson); }
    catch { throw new Error("MCP 配置不是有效的 JSON，请检查引号、逗号和括号"); }
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      throw new Error("请填写单个 MCP 连接的 JSON 对象");
    }
    // Field types, transport inference and target capabilities are validated by Rust.
    return value as AgentCatalogMcpInput;
  }
  function buildRequest(): AgentCatalogSaveRequest {
    return {
      assetId: props.definition?.assetId ?? null, expectedVersion: props.definition?.version ?? null,
      name: draft.name.trim(), category: draft.category,
      mcp: draft.category === "mcp" ? parseMcpJson() : null,
      skillMarkdown: draft.category === "skill" ? (draft.skillMarkdown || null) : null,
      hook: draft.category === "hook" ? { variants: draft.hookVariants.map((variant) => ({ ...variant })) } : null,
    };
  }
  const comparison = computed(() => {
    try { return { request: buildRequest(), error: "" }; }
    catch (error) { return { request: null, error: error instanceof Error ? error.message : String(error) }; }
  });
  function save(intent: "save" | "apply" = "save") {
    if (props.loading || props.saving || (props.editingAssetId && props.definition?.assetId !== props.editingAssetId)) return;
    validationError.value = "";
    if (!draft.name.trim()) { validationError.value = "请填写资产名称"; return; }
    if (draft.category === "hook") {
      if (!draft.hookVariants.length) { validationError.value = "请添加至少一个 Agent 的原生 Hook 定义"; return; }
      const incomplete = draft.hookVariants.find((variant) => !variant.event.trim() || !variant.groupJson.trim());
      if (incomplete) {
        activeHookAgent.value = incomplete.agentKind;
        validationError.value = "请填写此 Agent 的原生事件与规则 JSON";
        return;
      }
    }
    try {
      onSave(buildRequest(), intent);
    } catch (error) { validationError.value = error instanceof Error ? error.message : String(error); }
  }
  return { draft, originalMcpJson, editing, comparison, validationError, activeHookAgent, activeHookVariant, availableHookAgents, addHookVariant, removeHookVariant, save };
}
