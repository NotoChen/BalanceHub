import type { AgentConfigurationFormat } from "../stores/agent-configuration-types";

/** Normalize assignment spacing without rewriting values, comments, or multiline strings. */
function formatDotenv(text: string): string {
  const parts = text.split(/(\r?\n)/);
  let quote: string | null = null;
  let escaped = false;
  for (let index = 0; index < parts.length; index += 2) {
    const line = parts[index];
    let value = line;
    if (quote === null) {
      const assignment = line.match(/^\s*(export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$/);
      if (!assignment) continue;
      value = assignment[3];
      parts[index] = `${assignment[1] ? "export " : ""}${assignment[2]}=${value}`;
      if (!["'", '"', "`"].includes(value[0])) continue;
      quote = value[0];
      value = value.slice(1);
    }
    for (const character of value) {
      if (escaped) { escaped = false; continue; }
      if (character === "\\" && quote !== "'") { escaped = true; continue; }
      if (character === quote) { quote = null; break; }
    }
    escaped = false;
  }
  const formatted = parts.join("");
  return formatted && !formatted.endsWith("\n") ? formatted + (text.includes("\r\n") ? "\r\n" : "\n") : formatted;
}

export async function formatConfigurationText(format: AgentConfigurationFormat, text: string): Promise<string> {
  if (format === "dotenv") return formatDotenv(text);
  if (format === "toml") {
    const { Taplo } = await import("@taplo/lib");
    const taplo = await Taplo.initialize();
    return taplo.format(text, { options: { reorderKeys: false, indentString: "  ", columnWidth: 100, trailingNewline: true } });
  }
  const { format: formatText } = await import("prettier/standalone");
  const plugins = format === "markdown"
    ? [await import("prettier/plugins/markdown")]
    : await Promise.all([import("prettier/plugins/babel"), import("prettier/plugins/estree")]);
  return formatText(text, {
    parser: format === "markdown" ? "markdown" : "json",
    plugins,
    tabWidth: 2,
    printWidth: 100,
    endOfLine: "auto",
    proseWrap: "preserve",
    embeddedLanguageFormatting: "off",
  });
}
