import { EditorState } from "@codemirror/state";
import { HighlightStyle, StreamLanguage, syntaxHighlighting } from "@codemirror/language";
import { json } from "@codemirror/lang-json";
import { markdown } from "@codemirror/lang-markdown";
import { json as jsonWithComments } from "@codemirror/legacy-modes/mode/javascript";
import { toml } from "@codemirror/legacy-modes/mode/toml";
import { properties } from "@codemirror/legacy-modes/mode/properties";
import { yaml } from "@codemirror/legacy-modes/mode/yaml";
import { tags } from "@lezer/highlight";

export type CodeFormat = "json" | "jsonc" | "toml" | "dotenv" | "markdown" | "yaml" | "text";
export function codeLanguage(format: CodeFormat) {
  switch (format) {
    case "json": return json();
    case "jsonc": return StreamLanguage.define(jsonWithComments);
    case "toml": return StreamLanguage.define(toml);
    case "dotenv": return StreamLanguage.define(properties);
    case "markdown": return markdown();
    case "yaml": return StreamLanguage.define(yaml);
    default: return [];
  }
}
export function codeFormatForPath(path: string | null): CodeFormat {
  const name = path?.toLowerCase() ?? "";
  if (name.endsWith(".toml")) return "toml";
  if (name.endsWith(".jsonc")) return "jsonc";
  if (name.endsWith(".json")) return "json";
  if (name.endsWith(".md")) return "markdown";
  if (/\.ya?ml$/.test(name)) return "yaml";
  if (/(^|[/\\])\.env(?:\.|$)/.test(name)) return "dotenv";
  return "text";
}
export const codeHighlighting = syntaxHighlighting(HighlightStyle.define([
  { tag: [tags.propertyName, tags.attributeName, tags.definition(tags.variableName)], color: "var(--code-key)" },
  { tag: [tags.string, tags.special(tags.string), tags.quote], color: "var(--code-string)" },
  { tag: [tags.number, tags.bool, tags.atom, tags.null, tags.keyword], color: "var(--code-literal)" },
  { tag: [tags.comment, tags.meta], color: "var(--code-comment)" },
  { tag: [tags.operator, tags.punctuation], color: "var(--code-punctuation)" },
  { tag: [tags.heading, tags.strong], color: "var(--code-key)", fontWeight: "650" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.link, color: "var(--code-key)", textDecoration: "underline" },
]));
export const codePhrases = EditorState.phrases.of({
  Find: "查找", Replace: "替换", next: "下一个", previous: "上一个", all: "全选匹配",
  "match case": "区分大小写", regexp: "正则表达式", "by word": "全词匹配",
  replace: "替换", "replace all": "全部替换", close: "关闭查找", "Go to line": "跳转行号",
  go: "跳转", "current match": "当前匹配", "replaced $ matches": "已替换 $ 处",
  "replaced match on line $": "已替换第 $ 行匹配", "on line": "位于行", "Fold line": "折叠行",
  "Unfold line": "展开行", "to": "至", "folded code": "已折叠代码", "unfold": "展开",
});
