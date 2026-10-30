import assert from "node:assert/strict";
import test from "node:test";
import { formatFileSize } from "../src/utils/file-size.ts";

test("file sizes round to one decimal and never print a negative or missing size", () => {
  assert.equal(formatFileSize(0), "0 B");
  assert.equal(formatFileSize(-1), "0 B");
  assert.equal(formatFileSize(Number.NaN), "0 B");
  assert.equal(formatFileSize(12), "12 B");
  assert.equal(formatFileSize(2048), "2.0 KiB");
  assert.equal(formatFileSize(1024 * 1024 * 3.25), "3.3 MiB");
});
