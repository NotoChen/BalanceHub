import { computed, ref } from "vue";
import type { AgentCatalogMcpInput, AgentMcpFormRead } from "../stores/agent-catalog-types";
import { fieldDraft, fieldValue, mcpConnectionFields, mcpOptionFields, optionField, type McpField, type McpFieldDraft } from "../utils/mcp-form-fields";

export type McpAuthMode = "automatic" | "token" | "headers" | "oauth" | "custom";
export interface McpDraftState {
  transport: string; remoteTransport: string; auth: McpAuthMode; tokenSource: "value" | "environment";
  token: string; authorizationKey: string; fields: Record<string, McpFieldDraft>; options: Record<string, McpFieldDraft>;
  removed: string[]; transportChanged: boolean;
}
const oauthKey = (key: string) => key.startsWith("oauth.");
const headerKey = (key: string) => ["headerEnvironment", "headersHelper"].includes(key);
const authKey = (key: string) => oauthKey(key) || headerKey(key) || key === "bearerTokenEnvVar";

export function useMcpConnectionDraft() {
  const schema = ref<AgentMcpFormRead | null>(null);
  const state = ref<McpDraftState | null>(null);
  const specs = ref<McpField[]>([]);
  const touched = ref<string[]>([]);
  const attempted = ref(false);
  const baseline = ref("");
  const local = computed(() => state.value?.transport === "stdio");
  const rules = computed(() => new Map(schema.value?.fields.map((rule) => [rule.key, rule])));
  const available = (key: string) => {
    const rule = rules.value.get(key);
    return Boolean(rule?.supported && (local.value ? rule.local : rule.remote));
  };
  function hasValue(field: McpField, option: boolean) {
    const value = (option ? state.value?.options : state.value?.fields)?.[field.key];
    if (!value) return false;
    try { return fieldValue(field, value) !== undefined; } catch { return true; }
  }
  const retained = computed(() => {
    if (!state.value) return [];
    return [...mcpConnectionFields.map((field) => ({ field, option: false })), ...specs.value.map((field) => ({ field, option: true }))]
      .filter(({ field, option }) => {
        const original = option ? schema.value?.input.connectionOptions?.[field.key]
          : schema.value?.input[field.key as keyof AgentCatalogMcpInput];
        return original != null && !available(field.key) && hasValue(field, option) && !state.value!.removed.includes(field.key)
          && (!state.value!.transportChanged || !rules.value.has(field.key));
      });
  });
  const runtime = computed(() => mcpConnectionFields.filter((field) => ["cwd", "env"].includes(field.key) && available(field.key)));
  const runtimeOptions = computed(() => specs.value.filter((field) => available(field.key) && !authKey(field.key)));
  const oauthOptions = computed(() => specs.value.filter((field) => available(field.key) && oauthKey(field.key)));
  const headerOptions = computed(() => specs.value.filter((field) => available(field.key) && headerKey(field.key)));
  const tokenEnvironment = computed(() => specs.value.find((field) => field.key === "bearerTokenEnvVar" && available(field.key)) ?? null);

  function load(result: AgentMcpFormRead, restore?: McpDraftState, originalBaseline?: string) {
    schema.value = result;
    const input = result.input;
    const headers = { ...input.headers };
    const bearer = Object.entries(headers).find(([key, value]) => key.toLowerCase() === "authorization" && /^Bearer /i.test(value));
    if (bearer) delete headers[bearer[0]];
    const extras = input.connectionOptions ?? {};
    const oauth = Object.keys(extras).some(oauthKey);
    const dynamic = Object.keys(extras).some(headerKey);
    const credentials = [Boolean(bearer || extras.bearerTokenEnvVar), oauth, dynamic || Object.keys(headers).length > 0].filter(Boolean).length;
    const type = input.type ?? "http";
    specs.value = [...new Set([...mcpOptionFields.map((field) => field.key), ...Object.keys(extras)])].map((key) => optionField(key, extras[key]));
    const initial: McpDraftState = {
      transport: type, remoteTransport: type === "stdio" ? "http" : type,
      auth: credentials > 1 ? "custom" : bearer || extras.bearerTokenEnvVar ? "token" : oauth ? "oauth" : dynamic || Object.keys(headers).length ? "headers" : "automatic",
      tokenSource: extras.bearerTokenEnvVar ? "environment" : "value", token: bearer?.[1].slice(7) ?? "", authorizationKey: bearer?.[0] ?? "Authorization",
      fields: Object.fromEntries(mcpConnectionFields.map((field) => [field.key, fieldDraft(field, field.key === "headers" ? headers : input[field.key as keyof AgentCatalogMcpInput])])),
      options: Object.fromEntries(specs.value.map((field) => [field.key, fieldDraft(field, extras[field.key])])),
      removed: [], transportChanged: false,
    };
    baseline.value = originalBaseline ?? JSON.stringify(initial);
    state.value = restore ? JSON.parse(JSON.stringify(restore)) as McpDraftState : initial;
    touched.value = []; attempted.value = false;
  }
  const modified = computed(() => Boolean(state.value && JSON.stringify(state.value) !== baseline.value));
  const result = computed(() => {
    const draft = state.value;
    const errors: Record<string, string> = {};
    if (!draft) return { input: null, errors };
    const value: Record<string, unknown> = { type: draft.transport };
    const extras: Record<string, unknown> = {};
    const assign = (field: McpField, option: boolean) => {
      if (draft.removed.includes(field.key)) return;
      try {
        const entry = fieldValue(field, (option ? draft.options : draft.fields)[field.key]);
        if (entry !== undefined) (option ? extras : value)[field.key] = entry;
      } catch (error) { errors[field.key] = error instanceof Error ? error.message : String(error); }
    };
    for (const field of mcpConnectionFields) {
      if (!available(field.key)) continue;
      if (field.key === "headers" && draft.auth === "automatic") continue;
      assign(field, false);
    }
    for (const field of specs.value) {
      if (!available(field.key)) continue;
      if (oauthKey(field.key) && !["oauth", "custom"].includes(draft.auth)) continue;
      if (headerKey(field.key) && !["headers", "custom"].includes(draft.auth)) continue;
      if (field.key === "bearerTokenEnvVar" && !(draft.auth === "custom" || (draft.auth === "token" && draft.tokenSource === "environment"))) continue;
      assign(field, true);
    }
    for (const item of retained.value) assign(item.field, item.option);
    if (!local.value && (draft.auth === "custom" || (draft.auth === "token" && draft.tokenSource === "value")) && draft.token) {
      const headers = (value.headers ?? {}) as Record<string, string>;
      if (Object.keys(headers).some((key) => key.toLowerCase() === "authorization") || extras.bearerTokenEnvVar) {
        errors.token = "访问令牌有多个来源，请保留直接填写或环境变量中的一种；自定义 Authorization 也需移除重复项";
      } else value.headers = { ...headers, [draft.authorizationKey]: `Bearer ${draft.token}` };
    }
    const requiredKey = local.value ? "command" : "url";
    if (typeof value[requiredKey] !== "string" || !String(value[requiredKey]).trim()) errors[requiredKey] = local.value ? "请填写安装说明中的启动命令" : "请填写服务提供方给出的 MCP 地址";
    if (!local.value && draft.auth === "token") {
      if (draft.tokenSource === "value" && !draft.token.trim()) errors.token = "请填写访问令牌，或选择其他认证方式";
      if (draft.tokenSource === "environment" && !extras.bearerTokenEnvVar) errors.bearerTokenEnvVar = "请填写保存令牌的环境变量名";
    }
    if (!local.value && draft.auth === "headers" && !Object.keys((value.headers ?? {}) as object).length && !extras.headerEnvironment && !extras.headersHelper) errors.headers = "请按服务说明添加请求头，或选择由 Agent 处理认证";
    if (Object.keys(extras).length) value.connectionOptions = extras;
    return { input: value as AgentCatalogMcpInput, errors };
  });
  const displayErrors = computed(() => Object.fromEntries(Object.entries(result.value.errors).filter(([key]) => attempted.value || touched.value.includes(key))));
  function touch(key: string) { if (!touched.value.includes(key)) touched.value.push(key); }
  function setField(key: string, value: McpFieldDraft, option = false) {
    if (!state.value) return;
    (option ? state.value.options : state.value.fields)[key] = value;
    state.value.removed = state.value.removed.filter((removed) => removed !== key);
    touch(key);
  }
  function setTransport(value: string) {
    const currentTransport = state.value?.transport;
    if (!state.value || currentTransport === value) return;
    state.value.transport = value; state.value.transportChanged = value !== schema.value?.input.type;
    if (value !== "stdio") state.value.remoteTransport = value;
  }
  return { schema, state, specs, local, rules, available, retained, runtime, runtimeOptions, oauthOptions, headerOptions, tokenEnvironment,
    baseline, modified, result, displayErrors, attempted, touch, load, setField, setTransport };
}
