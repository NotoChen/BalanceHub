import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join, resolve } from "node:path";
import test from "node:test";
import {
  cargoInvocation,
  defaultTauriTargetDir,
  localCargoEnv,
  projectRoot,
} from "../scripts/build-cache.mjs";

test("local builds share defaults and normalize relative overrides at the project root", () => {
  const defaults = localCargoEnv({});
  assert.equal(defaults.CARGO_TARGET_DIR, defaultTauriTargetDir({}));
  assert.equal(defaults.CARGO_BUILD_JOBS, "2");
  assert.equal(defaults.CARGO_INCREMENTAL, "0");
  const explicit = localCargoEnv({
    CARGO_TARGET_DIR: " shared-target ", CARGO_BUILD_JOBS: "1", CARGO_INCREMENTAL: "1",
  });
  assert.equal(explicit.CARGO_TARGET_DIR, resolve(projectRoot, "shared-target"));
  assert.equal(explicit.CARGO_BUILD_JOBS, "1");
  assert.equal(explicit.CARGO_INCREMENTAL, "1");
});

test("Cargo space checks follow explicit target flags and keep cleanup available", () => {
  const build = cargoInvocation(["+stable", "--color", "always", "test", "--target-dir", "other-target"], {});
  assert.equal(build.env.CARGO_TARGET_DIR, resolve(projectRoot, "src-tauri/other-target"));
  assert.equal(build.requiresBuildSpace, true);
  assert.equal(cargoInvocation(["clean", "--target-dir=old-target"], {}).requiresBuildSpace, false);
  assert.equal(cargoInvocation(["fmt", "--check"], {}).requiresBuildSpace, false);
  assert.equal(cargoInvocation(["test", "--help"], {}).requiresBuildSpace, false);
  assert.equal(cargoInvocation(["run", "--", "--help"], {}).requiresBuildSpace, true);
  assert.equal(cargoInvocation(["run", "--", "--target-dir", "app-data"], {}).env.CARGO_TARGET_DIR,
    defaultTauriTargetDir({}));
});

function withFakeCargo(run: (env: NodeJS.ProcessEnv, root: string) => void) {
  const root = mkdtempSync(join(tmpdir(), "bh-build-entry-"));
  try {
    const bin = join(root, "bin");
    mkdirSync(bin);
    writeFileSync(join(bin, "cargo"), `#!${process.execPath}
process.stdout.write('BH_FAKE_CARGO ' + JSON.stringify({
  args: process.argv.slice(2), cwd: process.cwd(),
  target: process.env.CARGO_TARGET_DIR,
  jobs: process.env.CARGO_BUILD_JOBS, incremental: process.env.CARGO_INCREMENTAL,
}) + '\\n');
process.exit(Number(process.env.BALANCEHUB_TEST_EXIT || 0));
`, { mode: 0o755 });
    run({
      ...process.env,
      PATH: `${bin}${delimiter}${process.env.PATH || ""}`,
      CARGO_TARGET_DIR: join(root, "target with spaces"),
      CARGO_BUILD_JOBS: "", CARGO_INCREMENTAL: "",
      BALANCEHUB_MIN_FREE_GIB: "0",
      BALANCEHUB_TARGET_WARN_GIB: "1000000000",
      BALANCEHUB_TEST_EXIT: "0",
    }, root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function invoke(script: string, args: string[], env: NodeJS.ProcessEnv) {
  const result = spawnSync(process.execPath, [join(projectRoot, "scripts", script), ...args], {
    cwd: projectRoot, env, encoding: "utf8", timeout: 20_000,
  });
  assert.ifError(result.error);
  return result;
}

const posixOnly = { skip: process.platform === "win32" ? "POSIX fake executable fixture" : false };

test("Cargo entry passes a shared environment and does not create target during preflight", posixOnly, () => {
  withFakeCargo((env) => {
    const result = invoke("cargo.mjs", ["check", "--locked"], env);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    const line = result.stdout.split("\n").find((value) => value.startsWith("BH_FAKE_CARGO "));
    assert.ok(line);
    const child = JSON.parse(line.slice("BH_FAKE_CARGO ".length));
    assert.deepEqual(child.args, ["check", "--locked"]);
    assert.equal(child.cwd, join(projectRoot, "src-tauri"));
    assert.equal(child.target, env.CARGO_TARGET_DIR);
    assert.equal(child.jobs, "2");
    assert.equal(child.incremental, "0");
    assert.match(result.stdout, /构建前/);
    assert.match(result.stdout, /构建后/);
    assert.equal(existsSync(env.CARGO_TARGET_DIR!), false);
  });
});

test("low-space preflight blocks Cargo before it starts but permits cleanup", posixOnly, () => {
  withFakeCargo((env) => {
    env.BALANCEHUB_MIN_FREE_GIB = "1000000000";
    const build = invoke("cargo.mjs", ["test"], env);
    assert.equal(build.status, 1);
    assert.doesNotMatch(build.stdout, /BH_FAKE_CARGO/);
    assert.match(build.stderr, /磁盘可用空间不足/);
    const clean = invoke("cargo.mjs", ["clean"], env);
    assert.equal(clean.status, 0, clean.stderr);
    assert.match(clean.stdout, /BH_FAKE_CARGO/);
  });
});

test("Cargo child failures remain failures", posixOnly, () => {
  withFakeCargo((env) => {
    env.BALANCEHUB_TEST_EXIT = "17";
    assert.equal(invoke("cargo.mjs", ["check"], env).status, 17);
  });
});

test("Tauri build also stops at the low-space gate", posixOnly, () => {
  withFakeCargo((env) => {
    env.BALANCEHUB_MIN_FREE_GIB = "1000000000";
    const result = invoke("tauri.mjs", ["build"], env);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /磁盘可用空间不足/);
    assert.doesNotMatch(result.stdout, /BH_FAKE_CARGO/);
    assert.equal(existsSync(env.CARGO_TARGET_DIR!), false);
  });
});

test("doctor stops before build steps when space is insufficient", posixOnly, () => {
  withFakeCargo((env) => {
    env.BALANCEHUB_MIN_FREE_GIB = "1000000000";
    const result = invoke("doctor.mjs", [], env);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /磁盘可用空间不足/);
    assert.doesNotMatch(result.stdout, /\[doctor\]|BH_FAKE_CARGO/);
    assert.equal(existsSync(env.CARGO_TARGET_DIR!), false);
  });
});

test("doctor does not continue after an earlier command fails", posixOnly, () => {
  withFakeCargo((env, root) => {
    for (const [command, code] of [["git", 19], ["npm", 0]] as const) {
      writeFileSync(join(root, "bin", command), `#!${process.execPath}
process.stdout.write('BH_FAKE_${command.toUpperCase()}\\n');
process.exit(${code});
`, { mode: 0o755 });
    }
    const result = invoke("doctor.mjs", [], env);
    assert.equal(result.status, 1);
    assert.match(result.stdout, /BH_FAKE_GIT/);
    assert.doesNotMatch(result.stdout, /BH_FAKE_NPM|BH_FAKE_CARGO/);
    assert.match(result.stderr, /Git 差异检查失败，退出码 19/);
  });
});

test("static doctor remains available during low-space recovery", posixOnly, () => {
  withFakeCargo((env) => {
    env.BALANCEHUB_MIN_FREE_GIB = "1000000000";
    const result = invoke("doctor.mjs", ["--static-only"], env);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.match(result.stdout, /平台脚本变量检查通过/);
    assert.doesNotMatch(result.stdout, /\[doctor\]|BH_FAKE_CARGO/);
  });
});
