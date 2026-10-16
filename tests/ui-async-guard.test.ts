import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { extname, join, relative } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { parse } from "@vue/compiler-sfc";
import { NodeTypes, type RootNode, type TemplateChildNode } from "@vue/compiler-core";

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

test("the identity guard accepts stable scalar members, sentinels and boolean calls", () => {
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
    "draft.value.id !== set.id",
    "profile.cliKind === request.value.cliKind",
    "request.value.cliKind === profile.cliKind",
    "rule.disabledHere !== disabled.value.includes(rule.name)",
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
    "pending.value.context === captured.id",
    "captured.cliKind !== pending.value.context",
    "pending.value.context === captured.includes",
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
  // Stable IDs, CLI kind enums and lengths are scalar guards. A scalar member on the other
  // operand must not exempt a ref's nested object comparison.
  const nestedObject = (operand: string) => nestedRef.test(operand) && !/\.(?:length|id|cliKind)$/.test(operand);
  return [...text.matchAll(comparison)].filter((match) => {
    // includes(...) returns a boolean; the regex captures its member name,
    // not an object operand. Keep comparing a bare includes member guarded.
    const booleanCall = match[2].endsWith(".includes") && /^\s*\(/.test(text.slice(match.index! + match[0].length));
    return !booleanCall && (nestedObject(match[1]) || nestedObject(match[2])) && !scalarKeywords.has(match[1]) && !scalarKeywords.has(match[2]);
  });
}

test("only explicitly documented critical transactions may lock a modal", () => {
  const findings: string[] = [];

  for (const path of sourceFiles(sourceRoot).filter((path) => extname(path) === ".vue")) {
    const text = readFileSync(path, "utf8");
    for (const lock of modalLocks(text)) {
      findings.push(finding(path, text, lock.offset, lock.source));
    }
  }

  assert.deepEqual(
    findings,
    [],
    `普通异步操作不得锁死模态窗口；关键事务必须写明 balancehub-critical-modal-lock：\n${findings.join("\n")}`,
  );

  assert.deepEqual(modalLocks(`<template>
    <a-tag :closable="!disabled" />
    <a-modal :closable="!saving" />
    <a-drawer v-bind:mask-closable='!saving' />
  </template>`).map((lock) => lock.source), [":closable=\"!saving\"", "v-bind:mask-closable='!saving'"]);
  assert.deepEqual(modalLocks(`<template>
    <!-- balancehub-critical-modal-lock: installation cannot be interrupted -->
    <a-modal :closable="!installing" />
  </template>`), []);
});

function modalLocks(text: string) {
  const { descriptor, errors } = parse(text);
  assert.deepEqual(errors, []);
  const locks: { offset: number; source: string }[] = [];
  const modalTags = new Set(["a-modal", "a-drawer", "AModal", "ADrawer"]);
  const lockProperties = new Set(["closable", "mask-closable", "esc-to-close"]);
  function visit(node: RootNode | TemplateChildNode) {
    if (node.type !== NodeTypes.ROOT && node.type !== NodeTypes.ELEMENT) return;
    if (node.type === NodeTypes.ELEMENT && modalTags.has(node.tag)) {
      for (const prop of node.props) {
        if (prop.type !== NodeTypes.DIRECTIVE || prop.name !== "bind" ||
            prop.arg?.type !== NodeTypes.SIMPLE_EXPRESSION || !lockProperties.has(prop.arg.content)) continue;
        const offset = prop.loc.start.offset;
        if (text.slice(Math.max(0, offset - 500), offset).includes("balancehub-critical-modal-lock:")) continue;
        locks.push({ offset, source: prop.loc.source });
      }
    }
    node.children.forEach(visit);
  }
  if (descriptor.template?.ast) visit(descriptor.template.ast);
  return locks;
}

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
