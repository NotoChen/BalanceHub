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
