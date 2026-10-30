import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

const read = (path: string) => readFileSync(fileURLToPath(new URL(path, import.meta.url)), "utf8");

const board = read("../src/components/ProviderBoard.vue");
const workspace = read("../src/components/AppWorkspace.vue");
const app = read("../src/App.vue");
const card = read("../src/components/ProviderCard.vue");
const list = read("../src/components/provider-card/ProviderAppImportList.vue");

test("an export target travels from the card to the controller through every layer", () => {
  // 卡片 → 卡片板 → 工作区 → 控制器。每一层都要声明并转发同一条事件，少一层
  // 点击「导出到 Magpie」就什么也不会发生：事件在中间那层被静默丢掉。
  assert.match(
    card,
    /addProviderToApp: \(provider: Provider, target: ProviderAppImportTarget\) =>\s*emit\("addProviderToApp", provider, target\)/s,
  );
  assert.match(
    board,
    /addProviderToApp: \[provider: Provider, target: ProviderAppImportTarget\];/,
  );
  assert.match(
    board,
    /@add-provider-to-app="\(provider, target\) => emit\('addProviderToApp', provider, target\)"/,
  );
  assert.match(workspace, /@add-provider-to-app=/);
  assert.match(
    workspace,
    /emit\('addProviderToApp', provider, target\)/,
  );
  assert.match(app, /@add-provider-to-app="app\.addProviderToApp"/);
});

test("the export panel shows each target app once and nests CC Switch's agents", () => {
  // 只有 Agent 名的平铺清单读起来像七个互不相干的应用；先给应用抬头（图标 +
  // 名称），再挂各自的目标。
  assert.match(list, /import ccSwitchLogo from "\.\.\/\.\.\/assets\/logos\/cc-switch\.png";/);
  assert.match(list, /class="provider-app-import-app"/);
  assert.match(list, /<img :src="ccSwitchLogo" alt="" aria-hidden="true" \/>/);
  assert.match(list, /<strong>CC Switch<\/strong>/);
  // Agent 目标缩进在应用抬头下面，和 Magpie 那一行不再同级。
  const group = list.slice(list.indexOf('class="provider-app-import-group"'));
  assert.ok(group.indexOf('class="provider-app-import-agents"') < group.indexOf("v-for=\"option in ccSwitchAgents\""));
  assert.match(list, /\.provider-app-import-agents\s*\{[^}]*padding-left:\s*8px;/s);
  // 每个 CC Switch 目标都带着自己的 Agent 图标和名称。
  assert.match(group, /<BrandIcon :brand="option\.target\.agent" :size="14" \/>/);
  assert.match(group, /\{\{ option\.label \}\}/);
  // 平铺清单已经没有调用点，不再留着一个没人用的包裹层。
  assert.doesNotMatch(list, /provider-app-import-list/);
});
