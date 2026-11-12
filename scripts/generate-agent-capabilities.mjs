import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const snapshotPath = path.join(
  root,
  "crates/agent/src/services/agent_cli/capabilities.snapshot.json",
);
const outputPath = path.join(root, "docs/agent-capabilities.md");
const checkOnly = process.argv.includes("--check");

const columns = [
  ["temporaryLaunch", "临时启动"],
  ["modelSelection", "模型选择"],
  ["sessionHistory", "会话读取"],
  ["sessionSearch", "会话搜索"],
  ["sessionDetail", "会话详情"],
  ["sessionResume", "继续会话"],
  ["sessionName", "会话命名"],
  ["liveness", "测活"],
  ["defaultConfig", "默认配置"],
  ["assetInventory", "资产盘点"],
  ["hookInventory", "Hook 盘点"],
  ["runtimeObservation", "运行观察"],
];

const descriptors = JSON.parse(fs.readFileSync(snapshotPath, "utf8"));
const lines = [
  "# Agent 能力矩阵",
  "",
  "> 本文件由 `npm run docs:capabilities` 从",
  "> `crates/agent/src/services/agent_cli/capabilities.snapshot.json` 生成，请勿手工编辑。",
  "",
  "`✓` 表示该能力由 Rust 侧适配器提供，`—` 表示暂不支持。前端与文档使用同一份快照；能力变化必须先用",
  "`UPDATE_AGENT_CAPABILITIES=1 npm run cargo -- test -p balancehub-agent --lib capability_matrix_matches_snapshot`",
  "更新快照，再运行 `npm run docs:capabilities` 生成文档。",
  "",
  `| Agent | CLI 命令 | ${columns.map(([, label]) => label).join(" | ")} |`,
  `| --- | --- | ${columns.map(() => "---").join(" | ")} |`,
];

for (const descriptor of descriptors) {
  const marks = columns.map(([field]) => (descriptor.capabilities[field] ? "✓" : "—"));
  lines.push(
    `| ${descriptor.label}（\`${descriptor.kind}\`） | \`${descriptor.executable}\` | ${marks.join(" | ")} |`,
  );
}

const rendered = `${lines.join("\n")}\n`;

if (checkOnly) {
  const current = fs.existsSync(outputPath) ? fs.readFileSync(outputPath, "utf8") : "";
  if (current !== rendered) {
    console.error("Agent 能力文档已过期：运行 npm run docs:capabilities 重新生成。");
    process.exitCode = 1;
  } else {
    console.log("Agent 能力文档已是最新。");
  }
} else {
  fs.writeFileSync(outputPath, rendered);
  console.log(`已生成 ${path.relative(root, outputPath)}（${descriptors.length} 个 Agent）。`);
}
