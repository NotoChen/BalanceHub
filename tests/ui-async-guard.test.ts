import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { extname, join, relative } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));
const sourceRoot = join(projectRoot, "src");
const sourceExtensions = new Set([".ts", ".tsx", ".vue"]);

test("async stale-result guards do not compare nested ref values by object identity", () => {
  const findings: string[] = [];
  const directComparison = /\.value\.[A-Za-z_$][A-Za-z0-9_$]*\s*(?:===|!==)\s*([A-Za-z_$][A-Za-z0-9_$]*)\b/g;
  const reverseComparison = /\b([A-Za-z_$][A-Za-z0-9_$]*)\s*(?:===|!==)\s*[A-Za-z_$][A-Za-z0-9_$]*\.value\.[A-Za-z_$][A-Za-z0-9_$]*/g;
  const scalarKeywords = new Set(["null", "undefined", "true", "false"]);

  for (const path of sourceFiles(sourceRoot)) {
    const text = readFileSync(path, "utf8");
    for (const pattern of [directComparison, reverseComparison]) {
      pattern.lastIndex = 0;
      for (const match of text.matchAll(pattern)) {
        if (scalarKeywords.has(match[1])) continue;
        findings.push(finding(path, text, match.index ?? 0, match[0]));
      }
    }
  }

  assert.deepEqual(
    findings,
    [],
    `不要比较 Vue ref 内嵌值与局部对象身份，请改用 request ID/revision：\n${findings.join("\n")}`,
  );
});

test("only explicitly documented critical transactions may lock a modal", () => {
  const findings: string[] = [];
  const dynamicModalLock = /:(?:closable|mask-closable|esc-to-close)\s*=\s*"[^"]+"/g;
  const criticalMarker = "balancehub-critical-modal-lock:";

  for (const path of sourceFiles(sourceRoot).filter((path) => extname(path) === ".vue")) {
    const text = readFileSync(path, "utf8");
    for (const match of text.matchAll(dynamicModalLock)) {
      const contextStart = Math.max(0, (match.index ?? 0) - 500);
      if (text.slice(contextStart, match.index ?? 0).includes(criticalMarker)) continue;
      findings.push(finding(path, text, match.index ?? 0, match[0]));
    }
  }

  assert.deepEqual(
    findings,
    [],
    `普通异步操作不得锁死模态窗口；关键事务必须写明 balancehub-critical-modal-lock：\n${findings.join("\n")}`,
  );
});

function sourceFiles(directory: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...sourceFiles(path));
    } else if (entry.isFile() && sourceExtensions.has(extname(path))) {
      files.push(path);
    }
  }
  return files;
}

function finding(path: string, text: string, offset: number, expression: string) {
  const line = text.slice(0, offset).split("\n").length;
  return `${relative(projectRoot, path)}:${line} ${expression}`;
}
