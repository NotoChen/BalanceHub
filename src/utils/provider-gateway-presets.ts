export interface ProviderGatewayPreset {
  label: string;
  baseUrl: string;
}

/// 通用 OpenAI 兼容网关的入口地址。这里只负责填写地址和名称，协议能力仍由
/// Rust 侧的协议目录决定；新增预设时保持地址自带版本段（`/v1`、`/v4`），
/// 模型列表和真实调用会接着这个版本段拼接。
export const PROVIDER_GATEWAY_PRESETS: ProviderGatewayPreset[] = [
  { label: "OpenRouter", baseUrl: "https://openrouter.ai/api/v1" },
  { label: "DeepSeek", baseUrl: "https://api.deepseek.com/v1" },
  { label: "SiliconFlow", baseUrl: "https://api.siliconflow.cn/v1" },
  { label: "Moonshot", baseUrl: "https://api.moonshot.cn/v1" },
  { label: "Groq", baseUrl: "https://api.groq.com/openai/v1" },
  { label: "Together", baseUrl: "https://api.together.xyz/v1" },
  { label: "Fireworks", baseUrl: "https://api.fireworks.ai/inference/v1" },
  { label: "Mistral", baseUrl: "https://api.mistral.ai/v1" },
  { label: "xAI", baseUrl: "https://api.x.ai/v1" },
  { label: "阿里云百炼", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1" },
  { label: "Ollama", baseUrl: "http://127.0.0.1:11434/v1" },
  { label: "LM Studio", baseUrl: "http://127.0.0.1:1234/v1" },
];

export function gatewayPresetForBaseUrl(
  baseUrl: string,
): ProviderGatewayPreset | undefined {
  const normalized = normalizeBaseUrl(baseUrl);
  if (!normalized) return undefined;
  return PROVIDER_GATEWAY_PRESETS.find(
    (preset) => normalizeBaseUrl(preset.baseUrl) === normalized,
  );
}

export function applyProviderGatewayPreset(
  identity: { name: string; baseUrl: string },
  preset: ProviderGatewayPreset,
): void {
  identity.baseUrl = preset.baseUrl;
  const currentName = identity.name.trim();
  const followsAnotherPreset = PROVIDER_GATEWAY_PRESETS.some(
    (item) => item.label === currentName,
  );
  if (!currentName || followsAnotherPreset) {
    identity.name = preset.label;
  }
}

function normalizeBaseUrl(value: string) {
  return value.trim().replace(/\/+$/, "").toLowerCase();
}
