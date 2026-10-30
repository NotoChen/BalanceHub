import assert from "node:assert/strict";
import test from "node:test";
import { cpaLogLevels, parseCpaLogLine } from "../src/utils/cpa-log.ts";

test("a gateway line becomes its parts and keeps the message intact", () => {
  const line = parseCpaLogLine(
    "[2026-10-22 10:00:00] [ab12cd34] [warning] [server.go:42] upstream slow latency=900ms",
  );
  assert.equal(line.timestamp, "2026-10-22 10:00:00");
  assert.equal(line.requestId, "ab12cd34");
  // Logrus names warnings `warn`; a filter must not split them in two.
  assert.equal(line.level, "warn");
  assert.equal(line.source, "server.go:42");
  assert.equal(line.message, "upstream slow latency=900ms");
});

test("a line without a request does not offer a drill-down", () => {
  assert.equal(
    parseCpaLogLine("[2026-10-22 10:00:00] [--------] [info ] [main.go:1] ready")
      .requestId,
    "",
  );
  assert.equal(
    parseCpaLogLine("[2026-10-22 10:00:00] [nothexid] [info ] [main.go:1] ready")
      .requestId,
    "",
  );
});

test("a line the formatter did not produce stays readable as a whole", () => {
  const line = parseCpaLogLine("panic: runtime error");
  assert.equal(line.message, "panic: runtime error");
  assert.equal(line.level, "");
  assert.equal(line.requestId, "");
  assert.equal(line.timestamp, "");
});

test("levels are listed by severity rather than by how they appeared", () => {
  assert.deepEqual(cpaLogLevels(["debug", "error", "info", "warn"]), [
    "error",
    "warn",
    "info",
    "debug",
  ]);
  // An unknown level still gets a place after the known ones.
  assert.deepEqual(cpaLogLevels(["odd", "error"]), ["error", "odd"]);
});
