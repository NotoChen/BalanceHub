import { invoke } from "./transport.ts";
import type { AgentCliKind } from "../stores/provider-types";

export const agentDocumentationUrls: Record<AgentCliKind, string> = {
  codex: "https://developers.openai.com/codex/config-basic/",
  claudeCode: "https://code.claude.com/docs/en/settings",
  gemini: "https://geminicli.com/docs/reference/configuration/",
  grok: "https://docs.x.ai/build/settings",
  qwenCode: "https://qwenlm.github.io/qwen-code-docs/en/users/configuration/settings/",
  opencode: "https://opencode.ai/docs/config/",
  pi: "https://pi.dev/docs/latest/configuration",
  crush: "https://github.com/charmbracelet/crush#configuration",
  copilot: "https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/use-byok-models",
  droid: "https://docs.factory.com/docs/model-independence/byok",
  qoder: "https://docs.qoder.com/cli/settings-reference",
  codebuddy: "https://www.codebuddy.cn/docs/cli/models",
  openclaw: "https://docs.openclaw.ai/gateway/configuration",
  jcode: "https://jcode.sh/docs#providers",
  mothx: "https://github.com/oschina/mothx/blob/main/docs/en/configuration.md",
  kimiCode: "https://moonshotai.github.io/kimi-code/en/configuration/config-files.html",
  stepCode: "https://platform.stepfun.com/docs/zh/step-code/quickstart",
  deveco: "https://developer.huawei.com/consumer/cn/doc/harmonyos-guides/ide-deveco-code",
  mimoCode: "https://mimo.mi.com/docs/zh-CN/tokenplan/integration/mimo-code",
  atomcode: "https://atomcode.atomgit.com/docs/en/configuration.html",
  cline: "https://docs.cline.bot/provider-config/openai-compatible",
  primeAgent: "https://github.com/PrimeIntellect-ai/prime-agent",
  penguin: "https://github.com/Prism-Shadow/penguin-harness",
  goose: "https://goose-docs.ai/docs/guides/config-files",
  hermes: "https://hermes-agent.nousresearch.com/docs/user-guide/configuration",
  dsh: "https://www.dsh.so/news/dsh-quick-start",
  omp: "https://omp.sh/docs",
  morph: "https://github.com/quailyquaily/mistermorph/blob/master/docs/configuration.md",
  minimaxCode: "https://agent.minimax.io/docs/cli/quick-start",
  raycast: "https://manual.raycast.com/ai",
  omo: "https://ohmyopencode.org/docs",
};

export const agentInstallationUrls: Record<AgentCliKind, string> = {
  codex: "https://developers.openai.com/codex/cli/",
  claudeCode: "https://code.claude.com/docs/en/quickstart",
  gemini: "https://geminicli.com/docs/get-started/installation/",
  grok: "https://docs.x.ai/build/overview#install",
  qwenCode: "https://qwenlm.github.io/qwen-code-docs/en/users/quickstart/",
  opencode: "https://opencode.ai/docs/",
  pi: "https://pi.dev/docs/latest/quickstart",
  crush: "https://github.com/charmbracelet/crush#installation",
  copilot: "https://docs.github.com/en/copilot/how-tos/set-up/install-copilot-cli",
  droid: "https://docs.factory.com/docs/droid-cli/quickstart",
  qoder: "https://docs.qoder.com/cli/quickstart",
  codebuddy: "https://www.codebuddy.cn/docs/cli/quickstart",
  openclaw: "https://docs.openclaw.ai/install",
  jcode: "https://github.com/1jehuang/jcode#installation",
  mothx: "https://github.com/oschina/mothx/blob/main/docs/en/getting-started.md",
  kimiCode: "https://moonshotai.github.io/kimi-code/en/",
  stepCode: "https://platform.stepfun.com/docs/zh/step-code/quickstart",
  deveco: "https://developer.huawei.com/consumer/cn/doc/ide-deveco-code-install",
  mimoCode: "https://mimo.mi.com/docs/zh-CN/tokenplan/integration/mimo-code",
  atomcode: "https://atomcode.atomgit.com/docs/en/getting-started.html",
  cline: "https://docs.cline.bot/getting-started/installing-cline",
  primeAgent: "https://github.com/PrimeIntellect-ai/prime-agent#install",
  penguin: "https://github.com/Prism-Shadow/penguin-harness#install",
  goose: "https://goose-docs.ai/docs/getting-started/installation",
  hermes: "https://hermes-agent.nousresearch.com/docs/getting-started/installation",
  dsh: "https://www.dsh.so/",
  omp: "https://omp.sh/",
  morph: "https://github.com/quailyquaily/mistermorph",
  minimaxCode: "https://agent.minimax.io/docs/cli/quick-start",
  raycast: "https://manual.raycast.com/ai",
  omo: "https://ohmyopencode.org/docs",
};

export type AgentDocumentationPage = "configuration" | "installation";

/** Uses the installed opener plugin; its capability permits only these documentation URLs. */
export function openAgentDocumentation(kind: AgentCliKind, page: AgentDocumentationPage = "configuration") {
  const urls = page === "installation" ? agentInstallationUrls : agentDocumentationUrls;
  return invoke<void>("plugin:opener|open_url", { url: urls[kind] });
}
