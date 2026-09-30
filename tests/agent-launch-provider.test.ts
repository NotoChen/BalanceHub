import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import test, { after, before, type TestContext } from "node:test";
import { fileURLToPath } from "node:url";
import { compileScript, parse } from "@vue/compiler-sfc";
import { renderToString } from "@vue/server-renderer";
import {
  createRenderer, createSSRApp, defineComponent, h, nextTick, reactive, type Component,
} from "vue";
import { createServer, type ViteDevServer } from "vite";
import type { Provider, ProviderApiKeyOption, ProviderInput } from "../src/stores/provider-types.ts";
import { providerSiteAddressLabel } from "../src/utils/provider-display.ts";

const componentPath = fileURLToPath(new URL("../src/components/agent-workspace/AgentLaunchProviderModal.vue", import.meta.url));
const componentModuleId = `${componentPath}.inline-test.ts`;
const optionsPath = fileURLToPath(new URL("../src/components/agent-workspace/AgentProviderOptions.vue", import.meta.url));
const optionsModuleId = `${optionsPath}.inline-test.ts`;
let server: ViteDevServer;
let selector: Component;
let emptyDraft: () => ProviderInput;

before(async () => {
  server = await createServer({
    optimizeDeps: { noDiscovery: true, include: [] },
    server: { middlewareMode: true }, appType: "custom", logLevel: "silent",
    plugins: [{
      name: "agent-launch-provider-render-test",
      enforce: "pre",
      resolveId(id) {
        if (id === componentModuleId || id === optionsModuleId) return id;
        if (id === "./AgentProviderOptions.vue" || id === optionsPath) return optionsModuleId;
      },
      load(id) {
        if (id !== componentModuleId && id !== optionsModuleId) return;
        const path = id === optionsModuleId ? optionsPath : componentPath;
        const { descriptor } = parse(readFileSync(path, "utf8"), { filename: path });
        return compileScript(descriptor, { fs: { fileExists: existsSync, readFile: (path) => readFileSync(path, "utf8") }, id: "agent-launch-provider-render-test", inlineTemplate: true }).content;
      },
    }],
  });
  const [component, input] = await Promise.all([
    server.ssrLoadModule(componentModuleId),
    server.ssrLoadModule("/src/utils/provider-input.ts"),
  ]);
  selector = component.default;
  emptyDraft = input.emptyDraft;
});

after(async () => { await server?.close(); });

const ModalShell = defineComponent({
  inheritAttrs: false,
  props: {
    visible: Boolean, title: String, modalClass: String,
    closable: Boolean, maskClosable: Boolean, escToClose: Boolean, unmountOnClose: Boolean,
  },
  emits: ["cancel"],
  setup(props, { emit, slots }) {
    return () => props.visible ? h("section", {
      role: "dialog", class: props.modalClass, "aria-label": props.title,
      "data-mask-closable": props.maskClosable, "data-esc-to-close": props.escToClose,
      "data-unmount-on-close": props.unmountOnClose,
    }, [
      h("h2", props.title),
      props.closable ? h("button", {
        type: "button", "aria-label": "关闭中转站选择", onClick: () => emit("cancel"),
      }, "关闭") : null,
      slots.default?.(),
    ]) : null;
  },
});

function provider(id: string, accountName = "测试账户", username = "fixture-user"): Provider {
  const input = emptyDraft();
  return {
    revision: 1, displayLabel: "同名中转站", protocolLabel: "NewAPI", protocolDescription: "",
    authModeLabel: "账号密码", authModeDescription: "",
    identity: {
      ...input.identity, id, name: "同名中转站", baseUrl: "https://relay.example.test/team/v1/",
      displayName: accountName, username, userId: `account-${id}`, siteLogo: "",
    },
    auth: { ...input.auth, loginUsername: username },
    quota: {
      available: 12, used: 3, known: true, totalKnown: true, scope: "account", unlimited: false,
      perUnit: 1, displayType: "USD", currencySymbol: "$", currencyExchangeRate: 1,
    },
    capabilities: {
      checkInKnown: false, checkInSupported: false, checkInAuthModes: [],
      apiKeyManagementKnown: false, apiKeyManagementSupported: false,
      invitationKnown: false, invitationSupported: false, inviteLink: "", probedAt: null, availableModels: [],
    },
    cli: input.cli,
    automation: {
      ...input.automation, lastSyncedAt: null, lastCheckedInAt: null, lastCheckInUser: "", checkInRecords: [],
    },
    liveness: {
      ...input.liveness, promptCursor: 0, nextAt: null, records: [], runCount: 0,
      totalInputTokens: 0, totalOutputTokens: 0, totalTokens: 0, totalCostUsd: 0,
    },
    proxy: input.proxy, notification: input.notification,
    runtime: { ...input.runtime, status: "ok" },
    actions: {
      accountManagement: false, checkIn: false, checkedInToday: false,
      apiKeyManagement: false, invitation: false, refreshModelsOnly: false,
    },
  };
}

function keyOption(key: string, localName: string, tokenId: string): ProviderApiKeyOption {
  return {
    localId: `local-${tokenId}`, localName, name: "站点远程 Key 名称", key, maskedKey: "",
    keyAvailable: true, tokenId, userId: "cached-key-user", status: "enabled",
    usedQuota: 0, remainQuota: 10, usedQuotaRaw: 0, remainQuotaRaw: 10,
    unlimitedQuota: false, group: "default", crossGroupRetry: false, modelLimitsEnabled: false,
    modelLimits: [], allowIps: [], quotaDisplayType: "USD", currencySymbol: "$",
  };
}

async function renderSelector(providers: Provider[], visible = true, agentLabel = "Codex CLI") {
  const app = createSSRApp(selector, { visible, agentLabel, providers });
  app.component("a-modal", ModalShell);
  return renderToString(app);
}

interface RenderNode {
  type: string;
  text: string;
  props: Record<string, unknown>;
  children: RenderNode[];
  parent: RenderNode | null;
}

function node(type: string, text = ""): RenderNode {
  return { type, text, props: {}, children: [], parent: null };
}

function removeNode(child: RenderNode) {
  if (!child.parent) return;
  const siblings = child.parent.children;
  const index = siblings.indexOf(child);
  if (index >= 0) siblings.splice(index, 1);
  child.parent = null;
}

function insertNode(child: RenderNode, parent: RenderNode, anchor: RenderNode | null = null) {
  removeNode(child);
  child.parent = parent;
  const index = anchor ? parent.children.indexOf(anchor) : -1;
  if (index < 0) parent.children.push(child);
  else parent.children.splice(index, 0, child);
}

function descendants(root: RenderNode, predicate: (value: RenderNode) => boolean): RenderNode[] {
  return [ ...(predicate(root) ? [root] : []), ...root.children.flatMap((child) => descendants(child, predicate)) ];
}

function textContent(value: RenderNode): string {
  return value.text + value.children.map(textContent).join(" ");
}

function options(root: RenderNode) {
  return descendants(root, (value) => value.props.class === "agent-launch-provider-option");
}

function click(value: RenderNode | undefined) {
  assert.ok(value, "Expected an interactive rendered control");
  assert.equal(value.type, "button");
  assert.equal(value.props.type, "button");
  assert.ok(!value.props.disabled);
  const action = value.props.onClick;
  assert.equal(typeof action, "function");
  if (typeof action === "function") return action();
}

function mountSelector(t: TestContext, providers: Provider[], onSelect?: (id: string) => unknown) {
  const state = reactive({ providers, visible: true, agentLabel: "Codex CLI" });
  const selected: string[] = [];
  const closed: boolean[] = [];
  const renderer = createRenderer<RenderNode, RenderNode>({
    patchProp(value, key, _previous, next) { value.props[key] = next; },
    insert: insertNode, remove: removeNode,
    createElement: (type) => node(type), createText: (text) => node("#text", text),
    createComment: (text) => node("#comment", text),
    setText(value, text) { value.text = text; },
    setElementText(value, text) { value.text = text; value.children = []; },
    parentNode: (value) => value.parent,
    nextSibling(value) {
      const siblings = value.parent?.children ?? [];
      return siblings[siblings.indexOf(value) + 1] ?? null;
    },
    insertStaticContent(content, parent, anchor) {
      const value = node("#static", content);
      insertNode(value, parent, anchor);
      return [value, value];
    },
  });
  const root = node("root");
  const app = renderer.createApp(defineComponent({
    setup() {
      return () => h(selector, {
        ...state,
        onSelect(id: string) { selected.push(id); return onSelect?.(id); },
        onClose() { closed.push(true); state.visible = false; },
      });
    },
  }));
  app.component("a-modal", ModalShell);
  app.mount(root);
  t.after(() => app.unmount());
  return { state, root, selected, closed };
}

test("same-name stations display distinct synchronized account names, usernames and IDs", async () => {
  const first = provider("first", "开发账户", "dev-user");
  const second = provider("second", "发布账户", "release-user");
  const html = await renderSelector([first, second]);

  assert.equal((html.match(/class="agent-launch-provider-option"/g) ?? []).length, 2);
  for (const value of ["开发账户", "发布账户", "dev-user", "release-user", "account-first", "account-second", "NewAPI"]) {
    assert.ok(html.includes(value), value);
  }
  assert.ok(html.includes('title="https://relay.example.test/team/v1"'));
  assert.ok(html.includes("Codex CLI · 选择中转站"));
});

test("site addresses retain distinct safe paths while dropping URL credentials, query and fragment", async () => {
  const first = provider("first");
  const second = provider("second");
  first.identity.baseUrl = "https://url-reader:url-password@relay.example.test/team-a/v1/?api_key=query-secret#fragment-secret";
  second.identity.baseUrl = "https://relay.example.test/team-b/v1/";
  const before = structuredClone([first, second]);
  const html = await renderSelector([first, second]);

  assert.equal(providerSiteAddressLabel(first), "https://relay.example.test/team-a/v1");
  assert.equal(providerSiteAddressLabel(second), "https://relay.example.test/team-b/v1");
  for (const value of ["team-a/v1", "team-b/v1"]) assert.ok(html.includes(value));
  for (const secret of ["url-reader", "url-password", "query-secret", "fragment-secret"]) assert.ok(!html.includes(secret), secret);
  assert.deepEqual([first, second], before);
});

test("API Key stations show local key remarks and masked identities without account cache or credentials", async () => {
  const first = provider("key-a", "cached-account-name", "cached-account-login");
  first.auth.mode = "apiKey";
  first.identity.protocol = "api";
  first.protocolLabel = "通用 API Key";
  first.auth.apiKey = "sk-synthetic-private-value-A111";
  first.auth.apiKeyOptions = [keyOption(first.auth.apiKey, "构建专用", "token-a")];
  first.auth.accessToken = "private-access-token";
  first.auth.sessionCookie = "private-session-cookie";
  first.auth.loginPassword = "private-login-password";
  first.auth.refreshToken = "private-refresh-token";
  const second = provider("key-b");
  second.auth.mode = "apiKey";
  second.auth.apiKeyTokenId = "token-b";
  second.auth.apiKeyOptions = [keyOption("", "测试专用", "token-b")];
  second.auth.apiKeyOptions[0].maskedKey = "sk-••••••••B222";
  const html = await renderSelector([first, second]);

  for (const value of ["构建专用", "测试专用", "sk-syn••••••••A111", "通用 API Key", "B2"]) assert.ok(html.includes(value), value);
  for (const secret of [
    first.auth.apiKey, first.auth.accessToken, first.auth.sessionCookie,
    first.auth.loginPassword, first.auth.refreshToken,
    "cached-account-name", "cached-account-login", "account-key-a", "cached-key-user", "token-a", "token-b",
  ]) assert.ok(!html.includes(secret), secret);
});

test("known credentials are redacted from free-form labels and encoded URL path segments, including titles", async () => {
  const value = provider("redacted");
  value.auth.mode = "apiKey";
  value.auth.apiKey = "sk-synthetic-path/credential-A333";
  value.auth.loginPassword = "local-password-secret";
  value.identity.remark = `本地备注 ${value.auth.loginPassword}`;
  value.displayLabel = `中转站 ${value.auth.apiKey}`;
  value.identity.baseUrl = `https://relay.example.test/tenant/${encodeURIComponent(value.auth.apiKey)}/v1`;
  const html = await renderSelector([value]);

  assert.ok(html.includes("https://relay.example.test/tenant/••••/v1"));
  assert.ok(html.includes('title="中转站 ••••"'));
  assert.ok(html.includes('title="本地备注 ••••"'));
  for (const secret of [value.auth.apiKey, encodeURIComponent(value.auth.apiKey), value.auth.loginPassword]) {
    assert.ok(!html.includes(secret), secret);
  }
});

test("URL-derived automatic names hide authority credentials and query or fragment payloads in text and titles", async () => {
  const first = provider("url-label", "部署账户", "deployment-user");
  first.identity.baseUrl = "https://url-user:url-pass@relay.example.test?api_key=url-query-secret#url-fragment-secret";
  first.identity.name = "url-user:url-pass@relay.example.test?api_key=url-query-secret#url-fragment-secret";
  first.displayLabel = first.identity.name;
  const second = provider("url-name", "审核账户", "review-user");
  second.identity.baseUrl = first.identity.baseUrl;
  second.identity.name = first.identity.name;
  second.displayLabel = "";
  const before = structuredClone([first, second]);
  const html = await renderSelector([first, second]);

  assert.equal((html.match(/title="••••:••••@relay\.example\.test\?••••#••••"/g) ?? []).length, 2);
  assert.ok(html.includes('title="https://relay.example.test"'));
  for (const value of ["部署账户", "审核账户", "deployment-user", "review-user", "account-url-label", "account-url-name"]) {
    assert.ok(html.includes(value), value);
  }
  for (const secret of ["url-user", "url-pass", "url-query-secret", "url-fragment-secret"]) {
    assert.ok(!html.includes(secret), secret);
  }
  assert.deepEqual([first, second], before);
});

test("encoded URL credentials and sensitive parameter values are redacted from explicit labels and account metadata", async () => {
  const value = provider("url-encoded", "部署账户", "independent-login");
  value.identity.baseUrl = "https://url%2freader:url%3apassword@relay.example.test/team/v1?api_key=query%2fcredential&client_secret=client+password&team=team-blue#/login?access_token=fragment%2Ftoken&tab=overview";
  const secrets = [
    "url%2freader", "url%2Freader", "url/reader", "url%3apassword", "url%3Apassword", "url:password",
    "query%2fcredential", "query%2Fcredential", "query/credential", "client+password", "client%20password", "client password",
    "fragment%2ftoken", "fragment%2Ftoken", "fragment/token",
  ];
  value.displayLabel = `研发中转站 team-blue overview ${secrets.join(" ")}`;
  value.identity.displayName = "部署账户 url/reader";
  value.identity.username = "independent-login query/credential";
  value.identity.userId = "member-42 fragment/token";
  const before = structuredClone(value);
  const html = await renderSelector([value]);

  assert.ok(html.includes('title="https://relay.example.test/team/v1"'));
  assert.ok(html.includes('title="部署账户 ••••"'));
  assert.ok(html.includes('title="independent-login ••••"'));
  assert.ok(html.includes('title="ID：member-42 ••••"'));
  assert.ok(html.includes("研发中转站 team-blue overview"));
  for (const secret of secrets) assert.ok(!html.includes(secret), secret);
  assert.deepEqual(value, before);
});

test("URL secrets are removed from provider remarks, selected key notes and cached masked-key titles", async () => {
  const first = provider("url-remark");
  first.auth.mode = "apiKey";
  first.identity.baseUrl = "https://note-reader:note-password@relay.example.test/team/v1?token=query-note#opaque-fragment==";
  first.identity.remark = "本地用途 note-reader note-password query-note opaque-fragment==";
  first.displayLabel = first.identity.remark;
  const second = provider("url-key-note");
  second.auth.mode = "apiKey";
  second.identity.baseUrl = "https://sk-syn:key-password@relay.example.test/team/v1?api_key=key-query#/callback?refresh_token=key-fragment&view=details";
  second.auth.apiKeyTokenId = "url-key";
  second.auth.apiKeyOptions = [keyOption("", "发布专用 sk-syn key-password key-query key-fragment details", "url-key")];
  second.auth.apiKeyOptions[0].maskedKey = "sk-syn••••••••K555";
  const before = structuredClone([first, second]);
  const html = await renderSelector([first, second]);

  assert.ok(html.includes('title="本地用途 •••• •••• •••• ••••"'));
  assert.ok(html.includes('title="发布专用 •••• •••• •••• •••• details"'));
  assert.ok(html.includes('title="••••••••••••K555"'));
  assert.ok(html.includes('title="https://relay.example.test/team/v1"'));
  for (const secret of ["note-reader", "note-password", "query-note", "opaque-fragment==", "sk-syn", "key-password", "key-query", "key-fragment"]) {
    assert.ok(!html.includes(secret), secret);
  }
  assert.deepEqual([first, second], before);
});

test("selection emits stable provider IDs after reorder and object replacement using native buttons", async (t) => {
  const first = provider("first", "第一账户");
  const second = provider("second", "第二账户");
  const mounted = mountSelector(t, [first, second]);
  mounted.state.providers = [
    { ...second, revision: 2, identity: { ...second.identity, displayName: "第二账户更新" } },
    { ...first, revision: 3 },
  ];
  await nextTick();
  const choices = options(mounted.root);
  assert.equal(choices.length, 2);
  for (const choice of choices) {
    assert.equal(choice.type, "button");
    assert.equal(choice.props.type, "button");
    assert.notEqual(choice.props.tabindex, -1);
  }
  click(choices.find((choice) => textContent(choice).includes("第二账户更新")));
  click(choices.find((choice) => textContent(choice).includes("第一账户")));
  assert.deepEqual(mounted.selected, ["second", "first"]);
  assert.deepEqual(mounted.closed, []);
});

test("ordinary close remains available while a selection listener has an unfinished promise", async (t) => {
  let finish!: () => void;
  const pending = new Promise<void>((resolve) => { finish = resolve; });
  t.after(() => finish());
  const mounted = mountSelector(t, [provider("pending")], () => pending);
  assert.equal(click(options(mounted.root)[0]), undefined);
  await nextTick();
  const dialog = descendants(mounted.root, (value) => value.props.role === "dialog")[0];
  assert.ok(dialog);
  assert.equal(dialog.props["data-mask-closable"], true);
  assert.equal(dialog.props["data-esc-to-close"], true);
  assert.equal(dialog.props["data-unmount-on-close"], true);
  click(descendants(mounted.root, (value) => value.props["aria-label"] === "关闭中转站选择")[0]);
  await nextTick();
  assert.deepEqual(mounted.selected, ["pending"]);
  assert.equal(mounted.closed.length, 1);
  assert.equal(descendants(mounted.root, (value) => value.props.role === "dialog").length, 0);
});

test("empty and hidden selectors are explicit, and unsynchronized accounts can use their configured login", async () => {
  const empty = await renderSelector([]);
  assert.ok(empty.includes("尚未添加中转站"));
  assert.ok(empty.includes("请先在中转站视角添加中转站"));
  assert.ok(!empty.includes('class="agent-launch-provider-option"'));

  const value = provider("unsynced", "", "");
  value.auth.loginUsername = "local-login";
  value.identity.userId = "";
  const html = await renderSelector([value]);
  assert.ok(html.includes("local-login"));
  const hidden = await renderSelector([value], false);
  assert.ok(!hidden.includes("local-login"));
  assert.ok(!hidden.includes('role="dialog"'));
});

test("invalid URLs never echo raw input and long safe labels remain available in titles", async () => {
  const value = provider("long");
  for (const address of ["invalid-input-secret", "javascript:invalid-input-secret", "file:///invalid-input-secret"]) {
    value.identity.baseUrl = address;
    assert.equal(providerSiteAddressLabel(value), "地址格式异常");
    assert.ok(!(await renderSelector([value])).includes("invalid-input-secret"));
  }
  value.identity.baseUrl = " ";
  assert.equal(providerSiteAddressLabel(value), "地址未配置");
  value.displayLabel = "用于区分中转站的长名称".repeat(24);
  value.identity.baseUrl = `https://relay.example.test/${"safe-path/".repeat(30)}v1/`;
  const html = await renderSelector([value]);
  assert.ok(html.includes(`title="${value.displayLabel}"`));
  assert.ok(html.includes(`title="${providerSiteAddressLabel(value)}"`));
  assert.equal((html.match(/class="agent-launch-provider-option"/g) ?? []).length, 1);
});
