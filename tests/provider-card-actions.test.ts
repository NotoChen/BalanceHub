import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test, { type TestContext } from "node:test";
import { fileURLToPath } from "node:url";
import * as vue from "vue";
import { createRenderer, nextTick } from "vue";
import { loadSource } from "./helpers/load-source.ts";
import type { AgentCliKind } from "../src/stores/providers.ts";
import type { Provider } from "../src/stores/provider-types.ts";

const actionsPath = fileURLToPath(
  new URL("../src/components/provider-card/ProviderCardActions.vue", import.meta.url),
);

type SetupComponent = {
  setup: (props: any, context: any) => Record<string, any>;
};

function mountFooter(
  t: TestContext,
  props: {
    provider: Provider;
    boundProviderId?: string | null;
    switchableKinds?: AgentCliKind[];
  },
) {
  const { default: component } = loadSource<{ default: SetupComponent }>(
    "components/provider-card/ProviderCardActions.vue",
    {
      "@lucide/vue": {},
      "@arco-design/web-vue/es/icon": {},
      "../AgentCliIcon.vue": { default: {} },
      "../ProviderAuthIcon.vue": { default: {} },
      "../workspace-card/WorkspaceCardFooter.vue": { default: {} },
      "../workspace-card/WorkspaceCardActionGroup.vue": { default: {} },
      "../workspace-card/CardIconButton.vue": { default: {} },
      "./ProviderCardActionMenus.vue": { default: {} },
      "./ProviderCheckInAction.vue": { default: {} },
      "../../stores/cli-runtime": {
        useCliRuntimeStore: () => ({
          cliEnvironmentProbe: {},
          cliRuntime: {
            configs: props.boundProviderId
              ? [{ providerId: props.boundProviderId, cliKind: "codex" as AgentCliKind }]
              : [],
          },
        }),
      },
      "../../utils/cli-environment": {
        agentCliLabel: (_probe: unknown, kind: string) => kind,
        availableCliKinds: () => (props.switchableKinds ?? []) as AgentCliKind[],
      },
      "../../utils/oauth-cli": { oauthAgentCliKind: (vendor: string) => vendor },
      "../../utils/provider-display": {
        // 称呼只有一份实现，这里按同一规则给出，测试才能读到真实文案。
        providerCardScopeNoun: (value: Provider) =>
          value.actions.cpaGateway
            ? "网关"
            : value.actions.oauthAccount
              ? "账号"
              : "中转站",
        providerAuthModeDescription: () => "",
        providerDefaultApiKeyOption: () => null,
        providerApiKeyDisplayName: () => "",
      },
      "../../utils/provider-actions": {
        supportsCheckIn: () => false,
        providerCheckedInToday: () => false,
      },
    },
  );

  const renderer = createRenderer<object, object>({
    patchProp() {},
    insert() {},
    remove() {},
    createElement: () => ({}),
    createText: () => ({}),
    createComment: () => ({}),
    setText() {},
    setElementText() {},
    parentNode: () => null,
    nextSibling: () => null,
  });
  let state!: Record<string, any>;
  const events: unknown[][] = [];
  const app = renderer.createApp({
    setup() {
      state = component.setup(
        { provider: props.provider, interactive: true },
        { expose() {}, emit: (...args: unknown[]) => events.push(args) },
      );
      return () => null;
    },
  });
  app.mount({});
  t.after(() => app.unmount());
  return { state, events };
}

function unwrap<T>(state: Record<string, any>, key: string): T {
  const value = state[key];
  return (value && typeof value === "object" && "__v_isRef" in value
    ? value.value
    : value) as T;
}

function provider(
  overrides: {
    refreshError?: string | null;
    refreshPending?: boolean;
    source?: string;
  } = {},
): Provider {
  return {
    identity: { id: "oauth-fixture", name: "oauth-fixture", remark: "", baseUrl: "" },
    auth: {
      credentialRevision: 1,
      mode: "accessToken",
      apiKey: "",
      accessToken: "",
      sessionCookie: "",
      apiKeyOptions: [],
      oauth: {
        vendor: "codex",
        email: "person@example.test",
        source: overrides.source ?? "",
        scopes: [],
        refreshPending: overrides.refreshPending ?? false,
        refreshError: overrides.refreshError ?? null,
      },
    },
    capabilities: { subscription: null, availableModels: [] },
    automation: { refreshInterval: 0 },
    runtime: { enabled: true, status: "idle" },
    actions: { oauthAccount: true, cpaGateway: false, temporaryCli: false },
  } as unknown as Provider;
}

test("a broken renewal turns the footer's labelled action into the remedy", (t) => {
  const broken = mountFooter(t, { provider: provider({ refreshError: "令牌已被撤销" }) });
  assert.equal(unwrap(broken.state, "reauthRecommended"), true);
  assert.equal(unwrap(broken.state, "reauthTitle"), "续期失败，重新执行 OAuth 授权");
  assert.equal(unwrap(broken.state, "renewal").label, "续期失败");
  assert.match(unwrap(broken.state, "renewalTitle"), /令牌已被撤销/);

  const pending = mountFooter(t, { provider: provider({ refreshPending: true }) });
  assert.equal(unwrap(pending.state, "reauthRecommended"), true);
  assert.equal(unwrap(pending.state, "renewal").label, "续期待确认");
  assert.match(unwrap(pending.state, "reauthTitle"), /续期待确认/);

  const healthy = mountFooter(t, { provider: provider() });
  assert.equal(unwrap(healthy.state, "reauthRecommended"), false);
  assert.equal(unwrap(healthy.state, "renewal").label, "");
});

test("the footer names the agent running this card's credential", (t) => {
  // 健康卡片不占元信息位：类型由分组标题写，来源由授权面板写。
  assert.equal(unwrap(mountFooter(t, { provider: provider() }).state, "boundPill"), null);

  const bound = mountFooter(t, {
    provider: provider(),
    boundProviderId: "oauth-fixture",
  });
  assert.equal(unwrap(bound.state, "bound"), "codex");
  assert.equal(unwrap(bound.state, "boundTitle"), "codex 正在使用此账号的授权");
  // 底栏有切换按钮时，同一个 Agent 已经由按钮上的图标和 is-serving 标出，
  // 再放一枚胶囊只会把这一行挤成两行。
  const switchable = provider();
  (switchable.actions as Record<string, unknown>).defaultCliConfig = true;
  const withSwitch = mountFooter(t, {
    provider: switchable,
    boundProviderId: "oauth-fixture",
    switchableKinds: ["codex"],
  });
  assert.equal(unwrap(withSwitch.state, "switchControlVisible"), true);
  assert.equal(unwrap(withSwitch.state, "boundPill"), null);
});

test("an OAuth card switches its own agent from the footer, not from the menu", (t) => {
  const card = provider();
  (card.actions as Record<string, unknown>).defaultCliConfig = true;
  const single = mountFooter(t, {
    provider: card,
    switchableKinds: ["codex"],
  });
  assert.equal(unwrap(single.state, "singleSwitchKind"), "codex");
  assert.equal(unwrap(single.state, "canSwitchCliConfig"), true);
  // 底栏是纯图标：Agent 名字只出现在提示里，按钮本身不写文字。
  assert.equal(unwrap(single.state, "switchTitle"), "把 codex 的默认授权切换到此处");

  const serving = mountFooter(t, {
    provider: card,
    boundProviderId: "oauth-fixture",
    switchableKinds: ["codex"],
  });
  assert.equal(unwrap(serving.state, "switchTitle"), "codex 正在使用此账号的授权");
  assert.equal((serving.state.isServing as (kind: AgentCliKind) => boolean)("codex"), true);
});

test("the switch keeps the agent name in its tooltip, not in the footer row", (t) => {
  const card = provider();
  (card.actions as Record<string, unknown>).defaultCliConfig = true;
  const footer = mountFooter(t, {
    provider: card,
    switchableKinds: ["codex"],
  });
  // 底栏只有一个开关图标：Agent 名字由提示说全，不占掉这一行的宽度。
  assert.match(unwrap(footer.state, "switchTitle"), /codex/);
});

test("a gateway keeps the agent menu because it can serve several agents", (t) => {
  const gateway = provider();
  (gateway.auth as Record<string, unknown>).oauth = null;
  (gateway.actions as Record<string, unknown>).defaultCliConfig = true;
  const card = mountFooter(t, {
    provider: gateway,
    switchableKinds: ["codex", "claudeCode"],
  });
  assert.equal(unwrap(card.state, "singleSwitchKind"), null);
  assert.equal(unwrap(card.state, "canSwitchCliConfig"), true);
});

test("the same footer serves a relay card, and its wording follows the card type", (t) => {
  const relay = provider();
  (relay.auth as Record<string, unknown>).oauth = null;
  (relay.actions as Record<string, unknown>).oauthAccount = false;
  const card = mountFooter(t, { provider: relay, boundProviderId: "oauth-fixture" });
  assert.equal(unwrap(card.state, "noun"), "中转站");
  assert.equal(unwrap(card.state, "boundTitle"), "codex 正在使用此中转站的授权");
  // 中转站卡片没有 OAuth 账号，续期状态对它没有意义。
  assert.equal(unwrap(card.state, "reauthRecommended"), false);
});

const messages: string[] = [];
const accepted: unknown[] = [];
const loginCalls: unknown[][] = [];
let opened: string | null = null;

function loadActions(options: {
  start: (vendor: string, id: string | null) => Promise<unknown>;
}) {
  const { useProviderActions } = loadSource<{
    useProviderActions: (options: Record<string, unknown>) => Record<string, any>;
  }>("composables/useProviderActions.ts", {
    vue,
    "@arco-design/web-vue": {
      Message: {
        info: (value: unknown) => messages.push(`info:${String(value)}`),
        error: (value: unknown) => messages.push(`error:${String(value)}`),
        success: (value: unknown) => messages.push(`success:${String(value)}`),
        warning: (value: unknown) => messages.push(`warning:${String(value)}`),
        loading: () => ({ close() {} }),
      },
      Modal: { confirm: () => ({ close() {} }) },
    },
    "@tauri-apps/plugin-dialog": { save: async () => null },
    "../api/oauth-accounts": {
      startOAuthLogin: (...args: unknown[]) => {
        loginCalls.push(args);
        return options.start(String(args[0]), args[1] as string | null);
      },
      getOAuthAccount: async () => ({ exportOptions: [] }),
      exportOAuthAccount: async () => ({ instructions: "", warning: "" }),
      refreshAllCpa: async () => ({}),
      sendOAuthToCpa: async () => ({ instructions: "", warning: "" }),
    },
    "../stores/oauth-accounts": {
      useOAuthAccountStore: () => ({
        accept: (task: unknown) => accepted.push(task),
        openTask: (id: string) => {
          opened = id;
        },
      }),
    },
    "../utils/oauth-display": { oauthVendorLabel: (vendor: string) => vendor },
    "../utils/provider-api-key-options": { hasUsableProviderApiKey: () => true },
    "../utils/provider-display": { providerDisplayLabel: (p: Provider) => p.identity.name },
    "./useProviderCopyActions": {
      useProviderCopyActions: () => ({
        copyProviderUrl: async () => {},
        copyProviderSecret: async () => {},
        copyInviteLink: async () => {},
      }),
    },
    "./useProviderAppImport": {
      useProviderAppImport: () => ({ addProviderToApp: async () => {} }),
    },
    "./provider-credential-dialogs": { confirmAction: async () => true },
  });
  return useProviderActions({
    providers: { value: [] },
    refreshByIds: async () => {},
    openWorkspacePicker: () => {},
    probeCapabilities: async () => ({ provider: null, message: "" }),
    getInviteLink: async () => "",
    reload: async () => {},
    openEditProvider: () => {},
    checkInProviderAction: async () => {},
    openApiKeyManager: () => {},
    openAvailableModels: () => {},
    openUsage: () => {},
    openRequestLogs: () => {},
    openPasswordChange: () => {},
    openCheckInRecords: () => {},
    toggleProvider: async () => {},
    removeProvider: async () => {},
  });
}

test("re-authorizing opens the login task's own progress panel", async () => {
  messages.length = 0;
  accepted.length = 0;
  opened = null;
  loginCalls.length = 0;
  const actions = loadActions({
    start: async () => ({ id: "task-1", revision: 1 }) as unknown,
  });
  await actions.reauthorizeOAuthCard(provider());
  await nextTick();
  assert.deepEqual(loginCalls, [["codex", "oauth-fixture"]]);
  assert.deepEqual(accepted, [{ id: "task-1", revision: 1 }]);
  assert.equal(opened, "task-1", "the panel follows the task it just started");
  assert.deepEqual(messages, [], "no toast replaces the live progress");
});

test("a rejected re-authorization reports the failure without starting a task", async () => {
  messages.length = 0;
  accepted.length = 0;
  const actions = loadActions({
    start: async () => {
      throw new Error("浏览器启动失败");
    },
  });
  await actions.reauthorizeOAuthCard(provider());
  await nextTick();
  assert.deepEqual(accepted, []);
  assert.deepEqual(messages, ["error:Error: 浏览器启动失败"]);
});

test("a CPA card cannot start an OAuth login", async () => {
  messages.length = 0;
  loginCalls.length = 0;
  const actions = loadActions({
    start: async () => ({ id: "task-1", revision: 1 }) as unknown,
  });
  const gateway = provider();
  (gateway.auth as Record<string, unknown>).oauth = null;
  await actions.reauthorizeOAuthCard(gateway);
  assert.deepEqual(loginCalls, []);
});

test("one footer serves all four card types, in one fixed order", () => {
  const source = readFileSync(actionsPath, "utf8");
  const template = source.slice(source.indexOf("<template>"), source.lastIndexOf("</template>"));
  // 和中转站卡片同一条规则：图标就是图标，底栏不再附带文字说明。
  assert.doesNotMatch(template, /<template #label>/);
  assert.doesNotMatch(template, /provider-card-action-button/);
  assert.ok((template.match(/<CardIconButton/g)?.length ?? 0) >= 6);
  // 复制、数据、站点、应用导入各自成菜单，不再收进一张「更多」；默认 CLI
  // 由底栏的开关按钮负责，菜单里不重复。
  assert.match(template, /<ProviderCardActionMenus/);
  assert.doesNotMatch(template, /show-cli-menu|group-label=/);
  assert.doesNotMatch(template, /label="更多"/);
  // 日常动作在前、管理动作在后，顺序不随卡型改变。
  const order = [
    "launchTemporaryCli",
    "<ProviderCheckInAction",
    'tone="refresh"',
    "singleSwitchKind",
    "cpaRenewAll",
    "reauthRecommended",
    'tone="edit"',
    "toggleProvider",
    'tone="danger"',
    "<ProviderCardActionMenus",
  ].map((needle) => {
    const at = template.indexOf(needle);
    assert.ok(at >= 0, `底栏缺少 ${needle}`);
    return at;
  });
  assert.deepEqual(order, [...order].sort((a, b) => a - b));
  // 三个卡型专属动作仍由能力开关控制，不会在错误的卡上长出来。
  assert.match(template, /v-if="canLaunchCli"/);
  assert.match(template, /v-if="canCheckIn"/);
  assert.match(template, /v-if="provider\.actions\.cpaGateway"/);
  assert.match(template, /v-if="reauthRecommended"/);
});

test("the card renders one footer for both header families", () => {
  const card = readFileSync(
    fileURLToPath(new URL("../src/components/ProviderCard.vue", import.meta.url)),
    "utf8",
  );
  assert.doesNotMatch(card, /ManagedProviderActions/);
  assert.equal(card.match(/<ProviderCardActions/g)?.length, 1);
  // 统一底栏发出的每个事件都必须有人转发，否则那枚按钮点了没反应。
  for (const event of [
    "cpaRenewAll",
    "reauthorizeOauthCard",
    "launchTemporaryCli",
    "switchCliConfig",
    "checkIn",
  ]) {
    assert.match(card, new RegExp(`emit\\("${event}"`), `ProviderCard 没有转发 ${event}`);
  }
});
