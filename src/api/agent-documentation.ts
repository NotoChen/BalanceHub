import { invoke } from "@tauri-apps/api/core";
import type { AgentCliKind } from "../stores/provider-types";

export const agentDocumentationUrls: Record<AgentCliKind, string> = {
  codex: "https://developers.openai.com/codex/config-basic/",
  claudeCode: "https://code.claude.com/docs/en/settings",
  gemini: "https://geminicli.com/docs/reference/configuration/",
  grok: "https://docs.x.ai/build/settings",
};

export const agentInstallationUrls: Record<AgentCliKind, string> = {
  codex: "https://developers.openai.com/codex/cli/",
  claudeCode: "https://code.claude.com/docs/en/quickstart",
  gemini: "https://geminicli.com/docs/get-started/installation/",
  grok: "https://docs.x.ai/build/overview#install",
};

export type AgentDocumentationPage = "configuration" | "installation";

/** Uses the installed opener plugin; its capability permits only these documentation URLs. */
export function openAgentDocumentation(kind: AgentCliKind, page: AgentDocumentationPage = "configuration") {
  const urls = page === "installation" ? agentInstallationUrls : agentDocumentationUrls;
  return invoke<void>("plugin:opener|open_url", { url: urls[kind] });
}
