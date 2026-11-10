/**
 * Frontend-only Agent assets. Rust remains authoritative for names and
 * capabilities; adding an Agent's UI identity is intentionally one entry here.
 */
export const agentCliVisuals = {
  codex: {
    source: new URL("../assets/logos/codex.svg", import.meta.url).href,
    orbitColor: "color-mix(in srgb, var(--color-text-1) 90%, transparent)",
    orbitGlow: "color-mix(in srgb, var(--color-text-1) 30%, transparent)",
  },
  claudeCode: {
    source: new URL("../assets/logos/claude.svg", import.meta.url).href,
    orbitColor: "#d97757",
    orbitGlow: "rgba(217, 119, 87, 0.34)",
  },
  gemini: {
    source: new URL("../assets/logos/gemini.svg", import.meta.url).href,
    orbitColor: "#4285f4",
    orbitGlow: "rgba(66, 133, 244, 0.32)",
  },
  grok: {
    source: new URL("../assets/logos/grok.svg", import.meta.url).href,
    orbitColor: "color-mix(in srgb, var(--color-text-1) 88%, transparent)",
    orbitGlow: "color-mix(in srgb, var(--color-text-1) 28%, transparent)",
  },
  qwenCode: {
    source: new URL("../assets/logos/qwen.png", import.meta.url).href,
    orbitColor: "#615ced",
    orbitGlow: "rgba(97, 92, 237, 0.32)",
  },
  opencode: {
    source: new URL("../assets/logos/opencode.svg", import.meta.url).href,
    orbitColor: "color-mix(in srgb, var(--color-text-1) 88%, transparent)",
    orbitGlow: "color-mix(in srgb, var(--color-text-1) 28%, transparent)",
  },
  pi: {
    source: new URL("../assets/logos/pi.svg", import.meta.url).href,
    orbitColor: "#4d9abf",
    orbitGlow: "rgba(77, 154, 191, 0.32)",
  },
  crush: {
    source: new URL("../assets/logos/crush.png", import.meta.url).href,
    orbitColor: "#d94fd0",
    orbitGlow: "rgba(217, 79, 208, 0.32)",
  },
  copilot: {
    source: new URL("../assets/logos/copilot.svg", import.meta.url).href,
    orbitColor: "color-mix(in srgb, var(--color-text-1) 88%, transparent)",
    orbitGlow: "color-mix(in srgb, var(--color-text-1) 28%, transparent)",
  },
  droid: {
    source: new URL("../assets/logos/droid.svg", import.meta.url).href,
    orbitColor: "color-mix(in srgb, var(--color-text-1) 88%, transparent)",
    orbitGlow: "color-mix(in srgb, var(--color-text-1) 28%, transparent)",
  },
  qoder: {
    source: new URL("../assets/logos/qoder.svg", import.meta.url).href,
    orbitColor: "#4d6bfe",
    orbitGlow: "rgba(77, 107, 254, 0.32)",
  },
  codebuddy: {
    source: new URL("../assets/logos/codebuddy.svg", import.meta.url).href,
    orbitColor: "#6c4dff",
    orbitGlow: "rgba(108, 77, 255, 0.32)",
  },
  openclaw: {
    source: new URL("../assets/logos/openclaw.svg", import.meta.url).href,
    orbitColor: "#e5484d",
    orbitGlow: "rgba(229, 72, 77, 0.32)",
  },
  jcode: {
    source: new URL("../assets/logos/jcode.svg", import.meta.url).href,
    orbitColor: "color-mix(in srgb, var(--color-text-1) 88%, transparent)",
    orbitGlow: "color-mix(in srgb, var(--color-text-1) 28%, transparent)",
  },
  mothx: {
    source: new URL("../assets/logos/mothx.png", import.meta.url).href,
    orbitColor: "#7c5cff",
    orbitGlow: "rgba(124, 92, 255, 0.32)",
  },
  kimiCode: {
    source: new URL("../assets/logos/kimi.svg", import.meta.url).href,
    orbitColor: "color-mix(in srgb, var(--color-text-1) 88%, transparent)",
    orbitGlow: "color-mix(in srgb, var(--color-text-1) 28%, transparent)",
  },
  stepCode: {
    source: new URL("../assets/logos/stepfun-color.svg", import.meta.url).href,
    orbitColor: "#0f9f74",
    orbitGlow: "rgba(15, 159, 116, 0.32)",
  },
  deveco: {
    source: new URL("../assets/logos/deveco-color.svg", import.meta.url).href,
    orbitColor: "#007dff",
    orbitGlow: "rgba(0, 125, 255, 0.32)",
  },
  mimoCode: {
    source: new URL("../assets/logos/mimocode.svg", import.meta.url).href,
    orbitColor: "#ff6900",
    orbitGlow: "rgba(255, 105, 0, 0.32)",
  },
  atomcode: {
    source: new URL("../assets/logos/atomcode.png", import.meta.url).href,
    orbitColor: "#3b82f6",
    orbitGlow: "rgba(59, 130, 246, 0.32)",
  },
  cline: {
    source: new URL("../assets/logos/cline.svg", import.meta.url).href,
    orbitColor: "color-mix(in srgb, var(--color-text-1) 88%, transparent)",
    orbitGlow: "color-mix(in srgb, var(--color-text-1) 28%, transparent)",
  },
  primeAgent: {
    source: new URL("../assets/logos/prime-agent.svg", import.meta.url).href,
    orbitColor: "color-mix(in srgb, var(--color-text-1) 88%, transparent)",
    orbitGlow: "color-mix(in srgb, var(--color-text-1) 28%, transparent)",
  },
  penguin: {
    source: new URL("../assets/logos/penguin.svg", import.meta.url).href,
    orbitColor: "#0258f4",
    orbitGlow: "rgba(2, 88, 244, 0.32)",
  },
  goose: {
    source: new URL("../assets/logos/goose.svg", import.meta.url).href,
    orbitColor: "color-mix(in srgb, var(--color-text-1) 88%, transparent)",
    orbitGlow: "color-mix(in srgb, var(--color-text-1) 28%, transparent)",
  },
  hermes: {
    source: new URL("../assets/logos/hermes.svg", import.meta.url).href,
    orbitColor: "color-mix(in srgb, var(--color-text-1) 88%, transparent)",
    orbitGlow: "color-mix(in srgb, var(--color-text-1) 28%, transparent)",
  },
  dsh: {
    source: new URL("../assets/logos/deepseek-color.svg", import.meta.url).href,
    orbitColor: "#4d6bfe",
    orbitGlow: "rgba(77, 107, 254, 0.32)",
  },
  omp: {
    source: new URL("../assets/logos/omp.svg", import.meta.url).href,
    orbitColor: "color-mix(in srgb, var(--color-text-1) 88%, transparent)",
    orbitGlow: "color-mix(in srgb, var(--color-text-1) 28%, transparent)",
  },
  morph: {
    source: new URL("../assets/logos/mistermorph-color.svg", import.meta.url).href,
    orbitColor: "#2463ae",
    orbitGlow: "rgba(36, 99, 174, 0.32)",
  },
  minimaxCode: {
    source: new URL("../assets/logos/minimax-color.svg", import.meta.url).href,
    orbitColor: "#e2167e",
    orbitGlow: "rgba(226, 22, 126, 0.32)",
  },
  raycast: {
    source: new URL("../assets/logos/raycast.svg", import.meta.url).href,
    orbitColor: "#ff6363",
    orbitGlow: "rgba(255, 99, 99, 0.32)",
  },
  omo: {
    source: new URL("../assets/logos/omo.svg", import.meta.url).href,
    orbitColor: "color-mix(in srgb, var(--color-text-1) 88%, transparent)",
    orbitGlow: "color-mix(in srgb, var(--color-text-1) 28%, transparent)",
  },
  commandCode: {
    source: new URL("../assets/logos/command-code.svg", import.meta.url).href,
    orbitColor: "color-mix(in srgb, var(--color-text-1) 88%, transparent)",
    orbitGlow: "color-mix(in srgb, var(--color-text-1) 28%, transparent)",
  },
} as const;

export type AgentCliKind = keyof typeof agentCliVisuals;

export function hasAgentCliVisual(value: string): value is AgentCliKind {
  return Object.prototype.hasOwnProperty.call(agentCliVisuals, value);
}

/**
 * OAuth vendor keys differ from Agent CLI kinds only for Claude: Rust names the
 * Agent `claudeCode` while the OAuth vendor key is `claude`. Both show the same
 * mark, so the icon resolver accepts either key without a second logo table.
 */
const oauthVendorSources: Record<string, string> = {
  claude: agentCliVisuals.claudeCode.source,
};

export function brandSource(kind: string): string | null {
  if (hasAgentCliVisual(kind)) return agentCliVisuals[kind].source;
  return oauthVendorSources[kind] ?? null;
}
