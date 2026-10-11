import assert from "node:assert/strict";
import test from "node:test";
import {
  buildConfigurationDiff,
  configurationDiffRows,
  parseUnifiedDiff,
} from "../src/utils/configuration-text-diff.ts";
import { sessionHighlight } from "../src/utils/session-highlight.ts";

test("unified patches preserve both file line numbers and literal header-like changes", () => {
  const patch =
    "--- a/example.txt\n+++ b/example.txt\n@@ -12,2 +12,2 @@\n context\n--- old text\n+++ new text\n";
  const changes = parseUnifiedDiff(patch).lines.filter(
    (line) => line.kind !== "hunk",
  );
  assert.deepEqual(
    changes.map(({ kind, text, oldLine, newLine }) => ({
      kind,
      text,
      oldLine,
      newLine,
    })),
    [
      { kind: "context", text: "context", oldLine: 12, newLine: 12 },
      { kind: "removed", text: "-- old text", oldLine: 13, newLine: null },
      { kind: "added", text: "++ new text", oldLine: null, newLine: 13 },
    ],
  );
});

test("patches with unknown positions show changes without inventing line numbers", () => {
  const lines = parseUnifiedDiff(
    "*** Begin Patch\n*** Update File: example.ts\n@@\n-old\n+new\n*** End Patch",
  ).lines;
  assert.equal(lines.filter((line) => line.kind === "removed").length, 1);
  assert.equal(lines.filter((line) => line.kind === "added").length, 1);
  assert.ok(
    lines.every((line) => line.oldLine === null && line.newLine === null),
  );
});

test("a second file header does not inherit the previous hunk line numbers", () => {
  const lines = parseUnifiedDiff(
    "--- a/one\n+++ b/one\n@@ -1 +1 @@\n-old\n+new\n--- a/two\n+++ b/two\n@@ -8 +9 @@\n-two\n+second\n",
  ).lines;
  assert.equal(lines.find((line) => line.text === "--- a/two")?.kind, "hunk");
  assert.equal(lines.find((line) => line.text === "second")?.newLine, 9);
});

test("long patches retain the last change and original line numbers", () => {
  const content = Array.from({ length: 12_000 }, (_, index) =>
    index === 11_999
      ? "-原来的最后一行\n+修改后的最后一行"
      : ` 第 ${index + 1} 行`,
  ).join("\n");
  const lines = parseUnifiedDiff(
    `@@ -5001,12000 +7001,12000 @@\n${content}\n`,
  ).lines;
  assert.equal(lines.filter((line) => line.kind === "context").length, 11_999);
  assert.deepEqual(
    lines
      .slice(-2)
      .map(({ kind, text, oldLine, newLine }) => ({
        kind,
        text,
        oldLine,
        newLine,
      })),
    [
      {
        kind: "removed",
        text: "原来的最后一行",
        oldLine: 17_000,
        newLine: null,
      },
      {
        kind: "added",
        text: "修改后的最后一行",
        oldLine: null,
        newLine: 19_000,
      },
    ],
  );
});

test("folded ranges can restore every line and preserve expansion anchors", () => {
  const text = Array.from(
    { length: 2_000 },
    (_, index) => ` 第 ${index + 1} 行`,
  );
  text[100] = "-原文\n+新内容";
  text[1800] = "+新增一行\n 第 1801 行";
  const lines = parseUnifiedDiff(
    `@@ -1,2000 +1,2001 @@\n${text.join("\n")}\n`,
  ).lines;
  const rows = configurationDiffRows(lines, 3);
  assert.ok(rows.length < 30);
  assert.deepEqual(
    rows.flatMap((row) =>
      row.kind === "omitted"
        ? lines.slice(row.from, row.from + row.count)
        : [row],
    ),
    lines,
  );
  const omitted = rows.filter((row) => row.kind === "omitted");
  assert.ok(omitted.length >= 3);
  assert.ok(omitted.every((row) => lines[row.from]?.kind === "context"));
  assert.deepEqual(
    rows.filter((row) => row.kind === "added" || row.kind === "removed"),
    lines.filter((line) => line.kind === "added" || line.kind === "removed"),
  );
  assert.equal(
    configurationDiffRows(buildConfigurationDiff("same\n", "same\n").lines)[0]
      .kind,
    "omitted",
  );
});

test("text highlighting treats HTML and regular expression symbols as literal text", () => {
  const input = "<script>literal</script> a+b [x] 中文";
  const parts = sessionHighlight(input, ["<script>", "a+b", "[x]", "中文"]);
  assert.equal(parts.map((part) => part.text).join(""), input);
  assert.deepEqual(
    parts.filter((part) => part.matched).map((part) => part.text),
    ["<script>", "a+b", "[x]", "中文"],
  );
  assert.deepEqual(
    sessionHighlight("ſ S s", ["s"])
      .filter((part) => part.matched)
      .map((part) => part.text),
    ["ſ", "S", "s"],
  );
});
