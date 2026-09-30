// Presentation metadata only. Rust owns MCP inference, validation and conversion.
export interface McpField {
  key: string;
  label: string;
  kind: "text" | "list" | "pairs" | "number" | "json";
  help: string;
  placeholder?: string;
}
export const mcpConnectionFields: McpField[] = [
  { key: "url", label: "MCP 服务地址", kind: "text", help: "完整粘贴服务说明中的 MCP 地址，保留原有路径和查询参数。", placeholder: "https://服务域名/mcp" },
  { key: "command", label: "启动命令", kind: "text", help: "填写安装说明中的可执行程序，例如 npx、uvx、python，或程序的完整路径。", placeholder: "例如 npx" },
  { key: "args", label: "启动参数", kind: "list", help: "按安装说明逐项填写，每项一个参数。例如 npx -y 包名，应在这里添加 -y 和包名两项。", placeholder: "一个参数" },
  { key: "cwd", label: "工作目录", kind: "text", help: "程序需要在特定目录启动时填写；不确定时留空。", placeholder: "目录的完整路径（可选）" },
  { key: "env", label: "环境变量", kind: "pairs", help: "仅在安装说明要求时填写，例如 API_KEY 及服务提供方发放的密钥。" },
  { key: "headers", label: "请求头", kind: "pairs", help: "按服务说明填写，例如名称 X-API-Key、值为服务商提供的密钥；每行一项。" },
];
export const mcpOptionFields: McpField[] = [
  { key: "oauth.clientId", label: "OAuth 客户端 ID", kind: "text", help: "服务要求自定义 OAuth 应用时填写；通常由服务管理员提供。登录授权由目标 Agent 完成。" },
  { key: "oauth.scopes", label: "OAuth 权限范围", kind: "list", help: "按服务文档逐项填写，例如 read；服务未要求时留空。" },
  { key: "oauth.callbackPort", label: "OAuth 回调端口", kind: "number", help: "仅在服务要求固定本机回调端口时填写。" },
  { key: "oauth.callbackUrl", label: "OAuth 回调地址", kind: "text", help: "填写服务注册的完整回调地址；仅在服务要求时填写。" },
  { key: "bearerTokenEnvVar", label: "令牌环境变量名", kind: "text", help: "从环境变量读取访问令牌。这里填变量名，例如 MCP_TOKEN；不要填令牌本身。" },
  { key: "headerEnvironment", label: "请求头环境变量", kind: "pairs", help: "左侧填请求头名称，右侧填提供其值的环境变量名。" },
  { key: "environmentVariables", label: "继承的环境变量", kind: "list", help: "填写本地程序需要继承的环境变量名，每项一个。" },
  { key: "headersHelper", label: "动态请求头命令", kind: "text", help: "仅在服务要求动态生成认证信息时填写；由目标 Agent 执行此命令。" },
];
export const mcpTransportLabels: Record<string, string> = {
  http: "远程服务 · Streamable HTTP", stdio: "本地程序 · 标准输入输出",
  sse: "远程服务 · 旧版 SSE", webSocket: "远程服务 · WebSocket",
};
export function optionField(key: string, value: unknown): McpField {
  const known = mcpOptionFields.find((field) => field.key === key);
  // Preserve complex native values (e.g. remote environment references) exactly.
  const fits = known && (value == null
    || (known.kind === "text" && typeof value === "string")
    || (known.kind === "number" && typeof value === "number")
    || (known.kind === "list" && Array.isArray(value) && value.every((item) => typeof item === "string"))
    || (known.kind === "pairs" && typeof value === "object" && !Array.isArray(value) && Object.values(value).every((item) => typeof item === "string")));
  if (fits) return known;
  return { key, label: known?.label ?? key, kind: typeof value === "string" ? "text" : "json", help: "此扩展选项来自现有配置，按来源 Agent 的文档编辑。" };
}
export interface McpFieldDraft { text: string; items: string[]; pairs: { name: string; value: string }[] }
export function fieldDraft(field: McpField, value: unknown): McpFieldDraft {
  return {
    text: field.kind === "json" ? JSON.stringify(value ?? null, null, 2) : value == null ? "" : String(value),
    items: Array.isArray(value) ? value.map(String) : [],
    pairs: value && typeof value === "object" && !Array.isArray(value) ? Object.entries(value).map(([name, content]) => ({ name, value: String(content) })) : [],
  };
}
export function fieldValue(field: McpField, draft: McpFieldDraft): unknown {
  if (field.kind === "list") {
    if (draft.items.some((item) => !item.length)) throw new Error(`请填写“${field.label}”的空白项，或将其移除`);
    return draft.items.length ? [...draft.items] : undefined;
  }
  if (field.kind === "pairs") {
    const entries: [string, string][] = [];
    const names = new Set<string>();
    for (const row of draft.pairs) {
      if (!row.name.trim()) throw new Error(`请填写“${field.label}”的名称，或移除空白行`);
      if (names.has(row.name)) throw new Error(`“${field.label}”中 ${row.name} 重复，请保留一行`);
      names.add(row.name); entries.push([row.name, row.value]);
    }
    return entries.length ? Object.fromEntries(entries) : undefined;
  }
  if (!draft.text.length) return undefined;
  if (field.kind === "number") {
    const value = Number(draft.text);
    if (!draft.text.trim() || !Number.isFinite(value)) throw new Error(`“${field.label}”需要填写数字`);
    return value;
  }
  if (field.kind === "json") {
    try { return JSON.parse(draft.text); }
    catch { throw new Error(`“${field.label}”的 JSON 格式有误`); }
  }
  return draft.text;
}
