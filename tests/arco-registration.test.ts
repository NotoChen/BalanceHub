import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { loadSource } from "./helpers/load-source.ts";

function source(path: string) {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), "utf8");
}

/// Arco 组件的 install 会注册自己带前缀的子组件；标签解析到这些子组件时
/// 不需要单独列出，但必须知道它们归哪个父组件。
const childComponents: Record<string, string[]> = {
  Button: ["ButtonGroup"],
  Checkbox: ["CheckboxGroup"],
  Collapse: ["CollapseItem"],
  Dropdown: ["Doption"],
  Form: ["FormItem"],
  Input: ["InputGroup", "InputSearch", "InputPassword"],
  Radio: ["RadioGroup"],
  Select: ["Option", "Optgroup"],
  Skeleton: ["SkeletonLine"],
  Tabs: ["TabPane"],
};

/// 全局注册名来自 arco.ts：首屏直接注册的组件，以及按需家族里的组件。
/// 本地注册名来自组件自己的 `X as AX` 导入。
function globalNames() {
  const arco = source("../src/arco.ts");
  const eager = arco.match(/const eagerComponents: Plugin\[\] = \[([\s\S]*?)\n\];/);
  assert.ok(eager, "arco.ts 的组件表已改结构，测试需要同步");
  const eagerNames = [...eager[1].matchAll(/^\s*([A-Z]\w+),/gm)].map((match) => match[1]);
  assert.ok(eagerNames.length > 0);
  const lazyNames = [...arco.matchAll(/names: \[([^\]]*)\]/g)].flatMap((match) =>
    [...match[1].matchAll(/"(\w+)"/g)].map((entry) => entry[1]),
  );
  assert.ok(lazyNames.length > 0, "按需组件家族表已改结构，测试需要同步");
  const allowed = new Set<string>();
  for (const name of eagerNames) {
    allowed.add(`A${name}`);
    for (const child of childComponents[name] ?? []) allowed.add(`A${child}`);
  }
  for (const name of lazyNames) allowed.add(`A${name}`);
  return { eagerNames, lazyNames, allowed };
}

function localNames(markup: string) {
  const names = new Set<string>();
  for (const [, body] of markup.matchAll(
    /import\s*\{([^}]*)\}\s*from\s*"@arco-design\/web-vue"/g,
  )) {
    for (const entry of body.split(",")) {
      const parts = entry.trim().split(/\s+as\s+/);
      const bound = parts[1] ?? parts[0];
      if (bound) names.add(bound.trim());
    }
  }
  return names;
}

/// Vue 把 `<a-date-picker>` 解析成 `ADatePicker`：先 camelize，再首字母大写。
function resolvedName(tag: string) {
  const name = tag
    .slice(2)
    .replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase());
  return `A${name.charAt(0).toUpperCase()}${name.slice(1)}`;
}

function vueFiles(dir: string): string[] {
  const root = fileURLToPath(new URL(dir, import.meta.url));
  return readdirSync(root, { recursive: true, encoding: "utf8" })
    .filter((entry) => entry.endsWith(".vue"))
    .map((entry) => `${root}/${entry}`);
}

test("每个 a-* 标签都有全局或本地的组件注册", () => {
  const { eagerNames, allowed } = globalNames();
  const missing: string[] = [];
  for (const file of vueFiles("../src")) {
    const markup = readFileSync(file, "utf8");
    const local = localNames(markup);
    for (const [tag] of markup.matchAll(/<a-[a-z-]+[\s>/]/g)) {
      const name = resolvedName(tag.slice(0, -1));
      if (allowed.has(name) || local.has(name)) continue;
      missing.push(`${file.replace(/^.*\/src\//, "src/")} → <${tag.slice(1, -1)}> (${name})`);
    }
  }
  assert.deepEqual(
    [...new Set(missing)].sort(),
    [],
    `以下标签既不在 arco.ts 的组件表里，也没有在组件内以 A 前缀注册：\n${missing.join("\n")}`,
  );
  assert.ok(eagerNames.includes("Modal"), "Modal 等首屏组件仍应直接注册");
});

test("日期、时间和页签组件不再进入启动期的 Arco 包", () => {
  const { eagerNames, lazyNames } = globalNames();
  for (const name of ["DatePicker", "TimePicker", "InputTag", "Tabs"]) {
    assert.ok(
      !eagerNames.includes(name) && !lazyNames.includes(name),
      `${name} 只被按需面板使用，应改为组件内本地注册`,
    );
  }
});

/// 子组件的样式由父组件的 `style/css.js` 一并带入。
const styleOwners: Record<string, string> = {
  ButtonGroup: "button",
  CheckboxGroup: "checkbox",
  CollapseItem: "collapse",
  Doption: "dropdown",
  FormItem: "form",
  InputGroup: "input",
  InputPassword: "input",
  InputSearch: "input",
  Option: "select",
  Optgroup: "select",
  RadioGroup: "radio",
  SkeletonLine: "skeleton",
  TabPane: "tabs",
};

function styleDir(name: string) {
  return (
    styleOwners[name] ??
    name.replace(/([a-z0-9])([A-Z])/g, "$1-$2").toLowerCase()
  );
}

function styleImport(name: string) {
  return `es/${styleDir(name)}/style/css.js`;
}

test("每个首屏组件都按组件引入样式，按需组件样式跟随家族加载", () => {
  const { eagerNames, lazyNames } = globalNames();
  const styles = source("../src/arco-styles.ts");
  const imported = new Set(
    [...styles.matchAll(/@arco-design\/web-vue\/(es\/[^"]+\/style\/css\.js)/g)].map(
      (match) => match[1],
    ),
  );
  assert.doesNotMatch(
    source("../src/main.ts"),
    /@arco-design\/web-vue\/dist\/arco\.css/,
    "不能再整包引入 arco.css，否则未用到的组件样式仍会进首屏",
  );
  for (const name of eagerNames) {
    assert.ok(
      imported.has(styleImport(name)),
      `arco-styles.ts 缺少 ${name} 的 ${styleImport(name)}`,
    );
  }
  assert.ok(imported.has("es/message/style/css.js"), "Message 只走 JS API，样式必须单独引入");

  const arco = source("../src/arco.ts");
  const lazyStyles = new Set(
    [...arco.matchAll(/@arco-design\/web-vue\/(es\/[^"]+\/style\/css\.js)/g)].map(
      (match) => match[1],
    ),
  );
  for (const name of lazyNames) {
    assert.ok(
      lazyStyles.has(styleImport(name)),
      `arco.ts 的按需家族缺少 ${name} 的 ${styleImport(name)}`,
    );
    assert.ok(
      !imported.has(styleImport(name)),
      `${name} 已改为按需加载，样式不能继续留在 arco-styles.ts`,
    );
  }
});

test("组件内本地注册的 Arco 组件自带样式", () => {
  const { eagerNames, lazyNames } = globalNames();
  const global = new Set([...eagerNames, ...lazyNames]);
  for (const file of sourceFiles("../src")) {
    const markup = readFileSync(file, "utf8");
    for (const [, body] of markup.matchAll(
      /import\s*\{([^}]*)\}\s*from\s*"@arco-design\/web-vue"/g,
    )) {
      for (const entry of body.split(",")) {
        const trimmed = entry.trim();
        // 类型导入不参与运行时注册，也不需要样式。
        if (!trimmed || trimmed.startsWith("type ")) continue;
        const name = trimmed.split(/\s+as\s+/)[0];
        if (global.has(name) || name === "Message") continue;
        assert.match(
          markup,
          new RegExp(`@arco-design/web-vue/${styleImport(name).replace(/\./g, "\\.")}`),
          `${file.replace(/^.*\/src\//, "src/")} 本地注册了 ${name}，但没有引入 ${styleImport(name)}`,
        );
      }
    }
  }
});

function sourceFiles(dir: string): string[] {
  const root = fileURLToPath(new URL(dir, import.meta.url));
  return readdirSync(root, { recursive: true, encoding: "utf8" })
    .filter((entry) => entry.endsWith(".vue") || entry.endsWith(".ts"))
    .map((entry) => `${root}/${entry}`);
}

test("按需组件按模板解析名注册为异步组件", () => {
  const eager = Object.fromEntries(
    ["Alert", "Button", "ConfigProvider", "Dropdown", "Empty", "Modal", "Popover", "Progress", "Spin", "Tooltip"].map(
      (name) => [name, { name }],
    ),
  );
  const { registerArcoComponents } = loadSource<{
    registerArcoComponents: (app: unknown) => void;
  }>("arco.ts", { "@arco-design/web-vue": eager });
  const registered = new Map<string, unknown>();
  registerArcoComponents({
    use() {},
    component(name: string, component: unknown) {
      registered.set(name, component);
    },
  });
  // Vue 把 `<a-option>` 解析成 `AOption`，注册名必须与模板标签一一对应。
  for (const name of [
    "Select", "Option", "Optgroup", "Form", "FormItem",
    "Input", "InputGroup", "InputSearch", "InputPassword",
    "InputNumber", "Drawer", "Switch", "Textarea", "Tag",
    "Checkbox", "CheckboxGroup", "Radio", "RadioGroup",
    "Pagination", "Popconfirm", "Skeleton", "SkeletonLine",
    "Space", "Collapse", "CollapseItem",
  ]) {
    assert.ok(registered.has(`A${name}`), `缺少按需注册的 A${name}`);
  }
});

/// 按顶层逗号拆分选择器列表，`:is(a, b)` / `:not(a, b)` 里的逗号不算分隔。
function splitSelectorList(prelude: string) {
  const selectors: string[] = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index < prelude.length; index += 1) {
    const char = prelude[index];
    if (char === "(") depth += 1;
    else if (char === ")") depth = Math.max(0, depth - 1);
    else if (char === "," && depth === 0) {
      selectors.push(prelude.slice(start, index));
      start = index + 1;
    }
  }
  selectors.push(prelude.slice(start));
  return selectors.filter((selector) => selector.trim());
}

/// 按需组件的 Arco 样式会在应用样式之后加载；直接命中组件内部类名的
/// 单类选择器会被 Arco 默认值反超，必须加 `body` 等前缀提高优先级。
test("应用样式不再用单类选择器直接覆盖按需 Arco 组件", () => {
  const { lazyNames } = globalNames();
  // 本地注册的日期、时间、标签输入、页签和列表样式同样异步加载。
  const prefixes = [
    ...new Set([
      ...lazyNames.map((name) => `.arco-${styleDir(name)}`),
      ".arco-date-picker",
      ".arco-picker",
      ".arco-time-picker",
      ".arco-input-tag",
      ".arco-tabs",
      ".arco-list",
    ]),
  ];
  const styleDirRoot = fileURLToPath(new URL("../src/styles/modules", import.meta.url));
  const problems: string[] = [];
  for (const file of readdirSync(styleDirRoot, { recursive: true, encoding: "utf8" })) {
    if (!file.endsWith(".css")) continue;
    const path = `${styleDirRoot}/${file}`;
    const css = readFileSync(path, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
    for (const [, prelude] of css.matchAll(/([^{}]+)\{/g)) {
      if (prelude.trimStart().startsWith("@")) continue;
      for (const selector of splitSelectorList(prelude)) {
        const first = selector.trim().split(/[\s>+~]/)[0];
        if (!/^\.arco-[a-z-]+$/.test(first)) continue;
        // 选择器本身有更高优先级（多类、伪类、后代/子代）时不会被 Arco 默认值反超。
        if ((selector.match(/[.:]/g) ?? []).length > 1) continue;
        if (!prefixes.some((prefix) => first === prefix || first.startsWith(`${prefix}-`))) continue;
        problems.push(`${file} :: ${selector.trim().slice(0, 100)}`);
      }
    }
  }
  assert.deepEqual(
    problems,
    [],
    `以下选择器会被后加载的 Arco 样式覆盖，请加 body 前缀：\n${problems.join("\n")}`,
  );
});
