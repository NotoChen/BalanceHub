import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import test from "node:test";
import { loadSource } from "./helpers/load-source.ts";

// loadSource 通过 require 加载 pinia；测试里也从 require 取同一份实例，
// 才能让 setActivePinia 作用到被测 store。
const require = createRequire(import.meta.url);
const { createPinia, setActivePinia } = require("pinia") as typeof import("pinia");

type LayoutStore = {
  layout: "cards" | "list";
  layoutFor: (board: "providers" | "agents") => "cards" | "list";
  setLayout: (layout: "cards" | "list") => void;
  setBoardLayout: (board: "providers" | "agents", layout: "cards" | "list") => void;
};

function loadStore(): () => LayoutStore {
  const module = loadSource<{ useWorkspaceLayoutStore: () => LayoutStore }>(
    "stores/workspace-layout.ts",
    {},
  );
  return module.useWorkspaceLayoutStore;
}

function fakeStorage(options: {
  value?: string;
  legacy?: string;
  readError?: boolean;
  writeError?: boolean;
} = {}) {
  const data = new Map<string, string>();
  if (options.value != null) data.set("balancehub.workspace-layouts", options.value);
  if (options.legacy != null) data.set("balancehub.workspace-layout", options.legacy);
  return {
    data,
    getItem(key: string) {
      if (options.readError) throw new Error("storage blocked");
      return data.get(key) ?? null;
    },
    setItem(key: string, value: string) {
      if (options.writeError) throw new Error("storage blocked");
      data.set(key, value);
    },
  };
}

function createStore(storageOptions: Parameters<typeof fakeStorage>[0] = {}) {
  const storage = fakeStorage(storageOptions);
  const useStore = loadStore();
  const root = globalThis as typeof globalThis & { window?: unknown };
  const previous = root.window;
  root.window = { localStorage: storage };
  setActivePinia(createPinia());
  const store = useStore();
  return {
    store,
    storage,
    restore() {
      if (previous === undefined) delete root.window;
      else root.window = previous;
    },
  };
}

test("workspace layout defaults to cards and reads a stored list preference", () => {
  const empty = createStore();
  assert.equal(empty.store.layout, "cards");
  empty.restore();

  const stored = createStore({ value: JSON.stringify({ providers: "list" }) });
  assert.equal(stored.store.layout, "list");
  assert.equal(stored.store.layoutFor("agents"), "cards");
  stored.restore();

  // 未知值按默认卡片布局处理，不把异常状态带进界面。
  const unknown = createStore({ value: JSON.stringify({ providers: "grid" }) });
  assert.equal(unknown.store.layout, "cards");
  unknown.restore();

  // 未发布过的旧 key 不参与迁移；项目不保留不存在的历史兼容层。
  const legacy = createStore({ legacy: "list" });
  assert.equal(legacy.store.layout, "cards");
  assert.equal(legacy.store.layoutFor("agents"), "cards");
  legacy.restore();
});

test("switching layout persists the choice and survives storage failures", (t) => {
  const local = createStore();
  t.after(local.restore);
  local.store.setLayout("list");
  assert.equal(local.store.layout, "list");
  assert.deepEqual(
    JSON.parse(local.storage.data.get("balancehub.workspace-layouts") ?? "{}"),
    { providers: "list", agents: "cards" },
  );
  local.restore();

  const unreadable = createStore({ readError: true });
  assert.equal(unreadable.store.layout, "cards");
  unreadable.restore();

  const blocked = createStore({ writeError: true });
  blocked.store.setLayout("list");
  assert.equal(blocked.store.layout, "list");
  assert.equal(blocked.storage.data.has("balancehub.workspace-layouts"), false);
  blocked.restore();
});

test("each board keeps its own layout preference", (t) => {
  const created = createStore();
  t.after(created.restore);
  created.store.setBoardLayout("agents", "list");
  assert.equal(created.store.layoutFor("agents"), "list");
  assert.equal(created.store.layoutFor("providers"), "cards");
});

test("provider board switches between the card grid and a single-column list", () => {
  const board = readFileSync(
    new URL("../src/components/ProviderBoard.vue", import.meta.url),
    "utf8",
  );
  const toolbar = readFileSync(
    new URL("../src/components/ProviderBoardToolbar.vue", import.meta.url),
    "utf8",
  );
  const shared = readFileSync(
    new URL("../src/styles/modules/workspace-list.css", import.meta.url),
    "utf8",
  );
  const providerList = readFileSync(
    new URL("../src/styles/modules/provider-card/list.css", import.meta.url),
    "utf8",
  );
  const providerHeader = readFileSync(
    new URL("../src/components/provider-card/ProviderCardHeader.vue", import.meta.url),
    "utf8",
  );
  const providerCard = readFileSync(
    new URL("../src/components/ProviderCard.vue", import.meta.url),
    "utf8",
  );
  const providerBody = readFileSync(
    new URL("../src/components/provider-card/ProviderCardBody.vue", import.meta.url),
    "utf8",
  );
  const managedBody = readFileSync(
    new URL("../src/components/provider-card/ManagedProviderBody.vue", import.meta.url),
    "utf8",
  );

  assert.match(board, /useWorkspaceLayoutStore/);
  assert.match(board, /class="content provider-board"/);
  assert.match(board, /:class="\{ 'is-list': layout === 'list' \}"/);
  assert.match(board, /:layout="layout"/);
  assert.match(board, /@update:layout="layoutStore\.setLayout"/);
  assert.match(board, /:list="layout === 'list'"/);
  assert.match(toolbar, /WorkspaceLayoutSwitch/);
  assert.match(toolbar, /layout: WorkspaceLayout/);
  assert.match(toolbar, /"update:layout": \[layout: WorkspaceLayout\]/);
  assert.match(toolbar, /emit\('update:layout', \$event\)/);

  // 列表根节点仍是一张卡片，只是改成固定标题导轨、统一行高并取消固定卡片高度。
  assert.match(shared, /\.workspace-card\.workspace-card-list \{[\s\S]*?height:\s*var\(--workspace-list-row-height, 112px\);/);
  assert.match(shared, /\.workspace-card\.workspace-card-list \{[\s\S]*?var\(--workspace-list-title-width, clamp\(208px, 22vw, 272px\)\)/);
  assert.match(shared, /\.workspace-card\.workspace-card-list > \.workspace-card-header \{[\s\S]*?align-self:\s*stretch;/);
  assert.match(shared, /\.workspace-card\.workspace-card-list > \.workspace-card-content \{[\s\S]*?grid-template-columns:\s*minmax\(0, 1fr\) auto;/);
  assert.match(shared, /\.workspace-card\.workspace-card-list \.workspace-card-footer \{[\s\S]*?align-self:\s*stretch;/);
  // 窄窗口只统一放宽行高，不拆掉卡片之间的间距。
  assert.match(shared, /@media \(max-width: 1000px\)[\s\S]*?--workspace-list-row-height:\s*124px;/);
  // 四类中转站卡片（包含 OAuth 账号和 CPA 网关）共用同一份列表正文排布。
  assert.match(providerList, /\.provider-board\.is-list \.overview-provider-grid \{[\s\S]*?grid-template-columns:\s*minmax\(0, 1fr\);[\s\S]*?row-gap:\s*8px;/);
  assert.match(providerList, /\.provider-card\.workspace-card-list \.provider-card-body,\s*\n\.provider-card\.workspace-card-list \.managed-card-body \{[\s\S]*?flex-direction:\s*row;[\s\S]*?align-items:\s*flex-start;/);
  // 额度列整块垂直居中，和身份导轨、动作导轨共用同一条中线。
  assert.match(providerList, /\.provider-card\.workspace-card-list \.provider-card-body > \.provider-card-body-main,[\s\S]*?\.managed-card-body > \.provider-card-body-main \{[\s\S]*?align-self:\s*center;/);
  // 左列额度固定宽度，余额和比例条上下排；右列模型占剩余宽度并保留三行。
  assert.match(providerList, /\.provider-card\.workspace-card-list \.provider-card-body > \.provider-card-body-main \{\s*width:\s*220px;\s*max-width:\s*220px;\s*flex:\s*0 0 220px;/);
  assert.match(providerList, /\.provider-card\.workspace-card-list \.managed-card-body > \.provider-card-body-main \{\s*width:\s*280px;\s*max-width:\s*280px;\s*flex:\s*0 0 280px;/);
  assert.match(providerList, /\.provider-card\.workspace-card-list \.provider-card-quota \{[\s\S]*?flex-direction:\s*column;/);
  assert.match(providerList, /\.provider-card\.workspace-card-list \.provider-card-progress-row \{[\s\S]*?width:\s*100%;/);
  assert.match(providerList, /\.provider-card\.workspace-card-list \.provider-card-models \{[\s\S]*?flex:\s*1 1 0;[\s\S]*?flex-direction:\s*column;/);
  assert.match(providerList, /\.provider-card\.workspace-card-list \.provider-card-model-entry \{[\s\S]*?flex-direction:\s*column;/);
  // 列表不再把模型压成一行，也不再为「+N」芯片预留宽度。
  assert.doesNotMatch(providerBody, /:rows=/);
  assert.doesNotMatch(managedBody, /:rows=/);
  assert.match(providerBody, /:show-overflow="!list"/);
  assert.match(managedBody, /:show-overflow="!list"/);
  // 窗口进度条压细，行内数据不再把统一行高顶开。
  assert.match(providerList, /\.provider-card\.workspace-card-list \.account-quota-track \{[\s\S]*?height:\s*4px;/);
  // 列表标题导轨收纳账号的次级身份，正文不再重复同一行。
  assert.match(providerCard, /:list="list"/);
  assert.match(providerHeader, /providerIdentitySecondaryUsername/);
  assert.match(providerHeader, /class="provider-card-user-meta provider-card-header-account"/);
  assert.match(providerList, /\.provider-card\.workspace-card-list \.provider-card-identity \{[\s\S]*?display:\s*none;/);
  // 列表里状态文字保持可读，不只靠状态点颜色。
  assert.match(providerList, /\.provider-card\.workspace-card-list \.provider-card-status > span \{[\s\S]*?position:\s*static;[\s\S]*?clip-path:\s*none;/);
  // 动作按钮按 DOM 顺序折成两行，动作组不再各自占一条横轨。
  assert.match(providerList, /\.provider-card\.workspace-card-list \.workspace-card-quick-actions \{[\s\S]*?max-width:\s*calc\(6 \* 24px \+ 5 \* 2px\);[\s\S]*?flex-wrap:\s*wrap;/);
  assert.match(providerList, /\.provider-card\.workspace-card-list \.workspace-card-action-group \{[\s\S]*?display:\s*contents;/);
});

test("agent overview switches between cards and one card per row", () => {
  const dashboard = readFileSync(
    new URL("../src/components/agent-workspace/AgentDashboard.vue", import.meta.url),
    "utf8",
  );
  const card = readFileSync(
    new URL("../src/components/agent-workspace/AgentOverviewCard.vue", import.meta.url),
    "utf8",
  );
  const styles = readFileSync(
    new URL("../src/styles/modules/agent-workspace.css", import.meta.url),
    "utf8",
  );

  assert.match(dashboard, /useWorkspaceLayoutStore/);
  assert.match(dashboard, /WorkspaceLayoutSwitch/);
  assert.match(dashboard, /class="agent-overview-grid" :class="\{ 'is-list': layout === 'list' \}"/);
  assert.match(dashboard, /:list="layout === 'list'"/);
  assert.match(dashboard, /layoutStore\.setBoardLayout\('agents'/);
  assert.match(card, /list\?: boolean;/);
  assert.match(card, /'workspace-card-list': list/);
  // 列表仍是同一张卡片，只是网格退成一列；数据区再分成统计左列和配置文件右列。
  assert.match(styles, /\.agent-overview-grid\.is-list \{[\s\S]*?grid-template-columns:\s*minmax\(0, 1fr\);[\s\S]*?row-gap:\s*8px;/);
  // Agent 卡片之间同样保留间距，不共用连续表面。
  assert.match(styles, /\.workspace-card\.agent-overview-card\.workspace-card-list > \.workspace-card-content \{[\s\S]*?grid-template-columns:\s*240px minmax\(0, 1fr\) auto;[\s\S]*?grid-template-areas:\s*"statistics configuration actions";/);
  // 六项统计在左列排成 3 × 2：标签在上、数字在下，两行用满行高。
  assert.match(styles, /\.agent-overview-card\.workspace-card-list \.agent-card-statistics \{[\s\S]*?grid-template-columns:\s*repeat\(3, minmax\(0, 1fr\)\);[\s\S]*?align-content:\s*center;/);
  // 配置文件在右列：标题独占一行，芯片折两行，长列表不再挤成一条。
  assert.match(styles, /\.agent-overview-card\.workspace-card-list \.agent-card-configuration \{[\s\S]*?flex-direction:\s*column;/);
  assert.match(styles, /\.agent-overview-card\.workspace-card-list \.agent-card-file-list \{[\s\S]*?max-height:\s*44px;[\s\S]*?flex-wrap:\s*wrap;/);
  assert.match(styles, /\.agent-overview-card\.workspace-card-list \.agent-card-section-heading \{[\s\S]*?display:\s*flex;[\s\S]*?justify-content:\s*space-between;/);
  // 动作同样折成两行：配置和刷新在上，启动独占下一行。
  assert.match(styles, /\.agent-overview-card\.workspace-card-list \.agent-card-footer \.workspace-card-quick-actions \{[\s\S]*?grid-template-columns:\s*auto 28px;[\s\S]*?grid-auto-rows:\s*32px;/);
  assert.match(styles, /\.agent-overview-card\.workspace-card-list \.agent-card-footer \.workspace-card-action-group \{ display:\s*contents;/);
  assert.match(styles, /\.agent-overview-card\.workspace-card-list \.agent-card-footer \.agent-card-launch \{[\s\S]*?grid-column:\s*1 \/ -1;/);
});

test("the layout switch exposes both options as pressed buttons", () => {
  const source = readFileSync(
    new URL("../src/components/WorkspaceLayoutSwitch.vue", import.meta.url),
    "utf8",
  );
  assert.match(source, /\{ value: "cards", label: "卡片布局"/);
  assert.match(source, /\{ value: "list", label: "列表布局"/);
  assert.match(source, /role="group" aria-label="布局方式"/);
  assert.match(source, /:aria-pressed="props\.modelValue === option\.value"/);
  assert.match(source, /emit\('update:modelValue', option\.value\)/);
});
