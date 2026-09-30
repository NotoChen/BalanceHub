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

  for (const path of sourceFiles(sourceRoot)) {
    const text = readFileSync(path, "utf8");
    for (const match of nestedRefIdentityComparisons(text)) {
      findings.push(finding(path, text, match.index ?? 0, match[0]));
    }
  }

  assert.deepEqual(
    findings,
    [],
    `不要比较 Vue ref 内嵌值与局部对象身份，请改用 request ID/revision：\n${findings.join("\n")}`,
  );
});

test("the identity guard accepts complete length member comparisons and scalar sentinels", () => {
  for (const expression of [
    "preview.value.targets.length === selected.value.length",
    "selected.value.length !== preview.value.targets.length",
    "pending.value.items.length === expectedCount",
    "expectedCount !== pending.value.items.length",
    "pending.value.length === maximum",
    "pending.value.context === null",
    "undefined !== pending.value.context",
    "pending.value.available === false",
    "true !== pending.value.available",
  ]) {
    assert.deepEqual(nestedRefIdentityComparisons(expression), [], expression);
  }
});

test("the identity guard still rejects raw objects in both directions even when a local is named length", () => {
  for (const expression of [
    "pending.value.context === context",
    "pending.value.context !== context",
    "context === pending.value.context",
    "context !== pending.value.context",
    "pending.value.context === length",
    "length !== pending.value.context",
    "pending.value.context === captured.length",
    "captured.length !== pending.value.context",
    "pending.value.request.context === context",
    "context !== pending.value.request.context",
    "pending.value.context === captured.context",
    "captured.context !== pending.value.context",
    "pending.value.lengthContext === context",
    "pending.value.context === captured.value.context",
  ]) {
    const matches = nestedRefIdentityComparisons(`if (${expression}) return;`);
    assert.equal(matches.length, 1, expression);
    assert.equal(matches[0][0], expression);
    assert.equal(matches[0].index, 4);
  }
});

function nestedRefIdentityComparisons(text: string) {
  const identifier = "[A-Za-z_$][A-Za-z0-9_$]*";
  const member = `${identifier}(?:\\.${identifier})*`;
  // Match complete operands; a suffix such as targets.length is not a local named length.
  const comparison = new RegExp(`(?<![A-Za-z0-9_$.])(${member})\\s*(?:===|!==)\\s*(${member})(?![A-Za-z0-9_$.])`, "g");
  const nestedRef = new RegExp(`\\.value\\.${identifier}`);
  const scalarKeywords = new Set(["null", "undefined", "true", "false"]);
  // A length on the other operand must not exempt a ref's nested object comparison.
  const nestedObject = (operand: string) => nestedRef.test(operand) && !operand.endsWith(".length");
  return [...text.matchAll(comparison)].filter((match) =>
    (nestedObject(match[1]) || nestedObject(match[2])) && !scalarKeywords.has(match[1]) && !scalarKeywords.has(match[2]),
  );
}

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
