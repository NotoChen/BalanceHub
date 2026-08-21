import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

const componentPath = fileURLToPath(
  new URL("../src/components/BackgroundTaskIndicator.vue", import.meta.url),
);
const stylePath = fileURLToPath(
  new URL("../src/styles/modules/topbar.css", import.meta.url),
);

test("background task entry keeps its identity icon and uses color flow for activity", () => {
  const component = readFileSync(componentPath, "utf8");
  const styles = readFileSync(stylePath, "utf8");

  assert.match(component, /useId/);
  assert.match(component, /:color="activeCount > 0 \? `url\(#\$\{gradientId\}\)`/);
  assert.doesNotMatch(component, /LoaderCircle/);
  assert.doesNotMatch(component, /topbar-action-spin/);
  assert.match(styles, /@keyframes background-task-gradient-flow/);
  assert.match(styles, /@media \(prefers-reduced-motion: reduce\)/);
});
