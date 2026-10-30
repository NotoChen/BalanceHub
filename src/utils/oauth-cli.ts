import type { AgentCliKind } from "../agent-cli/visuals";
import type { OAuthVendor } from "../stores/oauth-types";

/**
 * A vendor's OAuth authorization is only accepted by that vendor's own CLI, so
 * an OAuth card never offers a CLI choice.
 */
export function oauthAgentCliKind(vendor: OAuthVendor): AgentCliKind {
  return vendor === "claude" ? "claudeCode" : vendor;
}
