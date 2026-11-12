import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const root = process.cwd();

/** Rust 分发表里每个命令读取的参数名。 */
function rustArguments(): Map<string, Set<string>> {
  const source = readFileSync(join(root, "crates/agent/src/api/mod.rs"), "utf8");
  const dispatch = source.slice(source.indexOf("async fn dispatch_inner"));
  const arms = dispatch.split(/^ {8}"([a-z0-9_]+)" =>/m);
  const result = new Map<string, Set<string>>();
  // split 把命令名放在奇数位，命令实现放在后一位。
  for (let index = 1; index < arms.length; index += 2) {
    const command = arms[index];
    const body = arms[index + 1] ?? "";
    const names = new Set<string>();
    for (const match of body.matchAll(/(?:arg|optional)\(args, "([A-Za-z0-9_]+)"\)/g)) {
      names.add(match[1]);
    }
    result.set(command, names);
  }
  return result;
}

/** 前端调用点传的参数名。 */
function frontendArguments(): Map<string, Set<string>> {
  const directory = join(root, "src/api");
  const result = new Map<string, Set<string>>();
  for (const file of readdirSync(directory)) {
    if (!file.endsWith(".ts")) continue;
    const source = readFileSync(join(directory, file), "utf8");
    for (const match of source.matchAll(/invoke(?:<[^>]*>)?\(\s*"([a-z0-9_]+)"/g)) {
      const command = match[1];
      const start = match.index + match[0].length;
      const rest = source.slice(start);
      const literal = firstArgumentObject(rest);
      const names = new Set<string>();
      for (const key of Object.keys(literal)) names.add(key);
      const previous = result.get(command);
      result.set(command, previous ? new Set([...previous, ...names]) : names);
    }
  }
  return result;
}

/** 取 invoke 的第二个参数对象字面量的顶层键；没有对象时返回空。 */
function firstArgumentObject(rest: string): Record<string, true> {
  const trimmed = rest.replace(/^\s*,?\s*/, "");
  if (!trimmed.startsWith("{")) return {};
  let depth = 0;
  let quote: string | null = null;
  let end = -1;
  for (let index = 0; index < trimmed.length; index += 1) {
    const char = trimmed[index];
    if (quote) {
      if (char === "\\") index += 1;
      else if (char === quote) quote = null;
      continue;
    }
    if (char === '"' || char === "'" || char === "`") {
      quote = char;
      continue;
    }
    if (char === "{" || char === "(" || char === "[") depth += 1;
    else if (char === "}" || char === ")" || char === "]") {
      depth -= 1;
      if (depth === 0) {
        end = index;
        break;
      }
    }
  }
  if (end < 0) return {};
  const body = trimmed.slice(1, end);
  const keys: Record<string, true> = {};
  let level = 0;
  let current = "";
  let innerQuote: string | null = null;
  const flush = () => {
    const key = current.split(":")[0].trim();
    if (key) keys[key] = true;
    current = "";
  };
  for (let index = 0; index < body.length; index += 1) {
    const char = body[index];
    if (innerQuote) {
      current += char;
      if (char === "\\") {
        current += body[index + 1] ?? "";
        index += 1;
      } else if (char === innerQuote) innerQuote = null;
      continue;
    }
    if (char === '"' || char === "'" || char === "`") {
      innerQuote = char;
      current += char;
      continue;
    }
    if (char === "{" || char === "(" || char === "[") level += 1;
    if (char === "}" || char === ")" || char === "]") level -= 1;
    if (char === "," && level === 0) {
      flush();
      continue;
    }
    current += char;
  }
  flush();
  return keys;
}

const camelCase = (name: string) =>
  name.replace(/_([a-z0-9])/g, (_, next: string) => next.toUpperCase());

test("前端传的 Agent 命令参数与 Rust 读取的参数一致", () => {
  const rust = rustArguments();
  const frontend = frontendArguments();
  assert.ok(rust.size > 100, `分发表只解析到 ${rust.size} 条命令`);
  // 进度通道由宿主注入，不属于分发表里的 JSON 参数。
  const hostChannelArguments = new Set(["progress"]);
  const problems: string[] = [];
  let compared = 0;
  for (const [command, names] of frontend) {
    const expected = rust.get(command);
    if (!expected) continue;
    compared += 1;
    const actual = new Set([...expected].map(camelCase));
    const missing = [...actual].filter((name) => !names.has(name));
    const extra = [...names].filter(
      (name) => !actual.has(name) && !hostChannelArguments.has(name),
    );
    if (missing.length || extra.length) {
      problems.push(
        `${command}: Rust 需要 [${[...actual].join(", ")}]，前端传 [${[...names].join(", ")}]`,
      );
    }
  }
  assert.deepEqual(problems, []);
  // 只解析到少量命令时这条断言会失效，这里固定一个下限。
  assert.ok(compared >= 90, `只比对了 ${compared} 条命令`);
});

test("前端调用的 Agent 命令都在分发表里登记", () => {
  const source = readFileSync(join(root, "crates/agent/src/api/mod.rs"), "utf8");
  const constant = source.slice(
    source.indexOf("pub const COMMANDS"),
    source.indexOf("];", source.indexOf("pub const COMMANDS")),
  );
  const registered = new Set(
    [...constant.matchAll(/"([a-z0-9_]+)"/g)].map((match) => match[1]),
  );
  const unknown = [...frontendArguments().keys()].filter(
    (command) => !registered.has(command),
  );
  // 只关心 Agent 域命令；核心命令由服务端自己的清单覆盖。
  const agentLike = unknown.filter(
    (command) => command.startsWith("agent_") || command.endsWith("_agent_session") ||
      command.includes("_agent_") || command.startsWith("plan_agent") ||
      command.startsWith("apply_agent") || command.startsWith("cancel_agent") ||
      command.startsWith("resume_agent") || command.startsWith("launch_temporary") ||
      command.startsWith("preview_temporary") || command.startsWith("get_temporary") ||
      command.startsWith("save_temporary") || command.startsWith("delete_temporary") ||
      command.includes("cli_") || command.startsWith("probe_cli") ||
      command.startsWith("inspect_agent") || command.startsWith("health_agent") ||
      command.startsWith("repair_agent") || command.startsWith("verify_agent") ||
      command.startsWith("estimate_session") || command.startsWith("browse_workspace") ||
      command.startsWith("forget_workspace") || command.startsWith("preview_cli_config") ||
      command.startsWith("get_wsl") || command.startsWith("start_wsl"),
    );
  assert.deepEqual(agentLike, []);
});
