import assert from "node:assert/strict";
import test from "node:test";

import {
  advanceProviderCardCliOrbitMotion,
  createProviderCardCliOrbitMotion,
  layoutProviderCardCliOrbits,
  PROVIDER_CARD_CLI_ORBIT_MAX_CYCLE_MS,
  PROVIDER_CARD_CLI_ORBIT_MAX_SPEED_PROGRESS_PER_SECOND,
  PROVIDER_CARD_CLI_ORBIT_MIN_CYCLE_MS,
  PROVIDER_CARD_CLI_ORBIT_MIN_SPEED_PROGRESS_PER_SECOND,
  PROVIDER_CARD_CLI_ORBIT_QUEUE_GAP_PROGRESS,
  PROVIDER_CARD_CLI_ORBIT_QUEUE_HEAD_PROGRESS,
  PROVIDER_CARD_CLI_ORBIT_QUEUE_MAX_SPAN_PROGRESS,
  providerCardCliOrbitSpec,
} from "../src/utils/provider-card-cli-orbit.ts";

for (const count of [1, 2, 3, 7, 20, 30]) {
  test(`provider card CLI orbits lay out ${count} agent slots`, () => {
    const kinds = ["codex", "claudeCode", "gemini", "grok"] as const;
    const specs = Array.from({ length: count }, (_, index) => ({
      ...providerCardCliOrbitSpec(kinds[index % kinds.length]),
      id: `agent-${index}`,
    }));
    const layouts = layoutProviderCardCliOrbits(specs);

    assert.equal(layouts.length, count);
    assert.equal(new Set(layouts.map((layout) => layout.phaseProgress)).size, count);

    for (const [index, layout] of layouts.entries()) {
      const gap =
        count <= 1
          ? 0
          : Math.min(
              PROVIDER_CARD_CLI_ORBIT_QUEUE_GAP_PROGRESS,
              PROVIDER_CARD_CLI_ORBIT_QUEUE_MAX_SPAN_PROGRESS / (count - 1),
            );
      const expectedProgress = PROVIDER_CARD_CLI_ORBIT_QUEUE_HEAD_PROGRESS - index * gap;
      assert.ok(Math.abs(layout.phaseProgress - expectedProgress) < 1e-9);
      assert.equal(
        layout.style["--provider-card-cli-orbit-progress"],
        `${layout.phaseProgress}%`,
      );
    }

    if (count > 1) {
      assert.ok(
        layouts[0].phaseProgress - layouts[layouts.length - 1].phaseProgress <=
          PROVIDER_CARD_CLI_ORBIT_QUEUE_MAX_SPAN_PROGRESS,
        "the agent train should stay compact instead of spanning opposite card edges",
      );
    }
  });
}

test("provider card CLI orbit specs reuse the central Agent visual registry", () => {
  assert.equal(providerCardCliOrbitSpec("codex").cliKind, "codex");
  assert.equal(providerCardCliOrbitSpec("claudeCode").cliKind, "claudeCode");
  assert.equal(providerCardCliOrbitSpec("gemini").cliKind, "gemini");
  assert.equal(providerCardCliOrbitSpec("grok").cliKind, "grok");
  assert.ok(providerCardCliOrbitSpec("gemini").color.includes("4285f4"));
});

test("adding an agent appends to the tail without moving existing icons", () => {
  const first = [providerCardCliOrbitSpec("codex"), providerCardCliOrbitSpec("claudeCode")];
  const two = layoutProviderCardCliOrbits(first);
  const three = layoutProviderCardCliOrbits([
    ...first,
    providerCardCliOrbitSpec("gemini"),
  ]);

  assert.equal(three[0].phaseProgress, two[0].phaseProgress);
  assert.equal(three[1].phaseProgress, two[1].phaseProgress);
  assert.notEqual(three[2].phaseProgress, three[1].phaseProgress);
});

test("each motion cycle re-rolls bounded duration and speed", () => {
  const values = [0, 0, 1, 1, 0.5, 0.5];
  let valueIndex = 0;
  const random = () => values[valueIndex++] ?? 0.5;
  const motion = createProviderCardCliOrbitMotion(20, random);

  assert.equal(motion.cycleDurationMs, PROVIDER_CARD_CLI_ORBIT_MIN_CYCLE_MS);
  assert.equal(
    motion.speedProgressPerSecond,
    PROVIDER_CARD_CLI_ORBIT_MIN_SPEED_PROGRESS_PER_SECOND,
  );

  advanceProviderCardCliOrbitMotion(motion, motion.cycleDurationMs, random);
  assert.equal(motion.cycleDurationMs, PROVIDER_CARD_CLI_ORBIT_MAX_CYCLE_MS);
  assert.equal(
    motion.speedProgressPerSecond,
    PROVIDER_CARD_CLI_ORBIT_MAX_SPEED_PROGRESS_PER_SECOND,
  );
  assert.equal(motion.cycleElapsedMs, 0);

  advanceProviderCardCliOrbitMotion(motion, motion.cycleDurationMs, random);
  assert.ok(
    motion.cycleDurationMs > PROVIDER_CARD_CLI_ORBIT_MIN_CYCLE_MS &&
      motion.cycleDurationMs < PROVIDER_CARD_CLI_ORBIT_MAX_CYCLE_MS,
  );
  assert.ok(
    motion.speedProgressPerSecond > PROVIDER_CARD_CLI_ORBIT_MIN_SPEED_PROGRESS_PER_SECOND &&
      motion.speedProgressPerSecond < PROVIDER_CARD_CLI_ORBIT_MAX_SPEED_PROGRESS_PER_SECOND,
  );
});
