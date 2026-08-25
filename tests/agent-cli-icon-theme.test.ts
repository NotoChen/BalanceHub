import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

const foundationPath = fileURLToPath(
  new URL("../src/styles/modules/foundation.css", import.meta.url),
);

test("dark mode keeps Codex orbit icons readable", () => {
  const foundation = readFileSync(foundationPath, "utf8");

  assert.match(foundation, /:root\.theme-dark \.agent-cli-icon-codex/);
  assert.match(foundation, /filter:\s*invert\(1\)/);
});
