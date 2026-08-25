import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

function source(path: string) {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), "utf8");
}

test("the provider editor owns the API Key vault inline instead of stacking another modal", () => {
  const credentials = source(
    "../src/components/provider-editor/ProviderEditorCredentialsSection.vue",
  );
  const vault = source("../src/components/provider-editor/ProviderApiKeyVault.vue");
  const editor = source("../src/components/ProviderEditorDrawer.vue");
  const overlays = source("../src/components/AppOverlays.vue");
  const app = source("../src/App.vue");
  const controller = source("../src/composables/useAppController.ts");

  assert.match(credentials, /import ProviderApiKeyVault/);
  assert.match(credentials, /<ProviderApiKeyVault/);
  assert.doesNotMatch(credentials, /manage-api-keys/);
  assert.match(editor, /apiKeyManagerProvider/);
  assert.match(editor, /<ProviderEditorCredentialsSection/);
  assert.doesNotMatch(overlays, /ApiKeyManager|api-key-manager/);
  assert.match(app, /@open-api-key-create-panel="app\.openApiKeyCreatePanel"/);
  assert.match(controller, /providerEditor\.openEditProvider\(provider, "credentials"\)/);
  assert.match(credentials, /:remote-managed="apiKeyRemoteManaged"/);
  assert.doesNotMatch(credentials, /:remote-managed="currentProtocol\?\.capabilities\.apiKeyManagement/);
  assert.match(vault, /当前调用 Key/);
  assert.match(vault, /新增、备注、切换和删除会立即保存/);
  assert.match(credentials, /showAuthModePicker/);
});

test("CLI default configuration remains editable for the currently bound API Key", () => {
  const picker = source("../src/components/CliConfigKeyPickerModal.vue");
  const runtime = source("../src/composables/useCliRuntime.ts");

  assert.match(picker, /当前使用 · 查看并编辑配置/);
  assert.doesNotMatch(picker, /:disabled="isCurrentAgentKey\(option\)"/);
  assert.match(
    runtime,
    /await previewProviderCliConfig\(provider, cliKind, option\);\s*if \(cliConfigPreviewVisible\.value\) \{\s*cliConfigKeyPickerVisible\.value = false;/s,
  );
  assert.match(runtime, /cliConfigRequestRevision \+= 1;/);
  assert.match(
    runtime,
    /options\.previewConfig\(provider\.identity\.id, cliKind, apiKey\.localId\.trim\(\)\)/,
  );
  assert.doesNotMatch(runtime, /已在使用这把 API Key/);
});

test("CLI preview and switch preserve the selected stable API Key identity", () => {
  const runtime = source("../src/composables/useCliRuntime.ts");
  const api = source("../src/api/app.ts");

  assert.match(
    runtime,
    /options\.switchConfig\(\s*preview\.providerId,\s*preview\.cliKind,\s*preview\.apiKeyLocalId,/s,
  );
  assert.match(
    runtime,
    /if \(requestRevision === cliConfigRequestRevision\) \{\s*store\.cliRuntime = runtime;/s,
  );
  assert.match(
    api,
    /invoke<CliConfigPreview>\("preview_cli_config", \{ id, cliKind, apiKeyLocalId \}\)/,
  );
  assert.match(api, /invoke<CliRuntimeSnapshot>\("switch_cli_config", \{/);
  assert.match(api, /apiKeyLocalId,/);
});

test("the key manager exposes card-default and Agent-specific bindings", () => {
  const manager = source("../src/components/provider-editor/ProviderApiKeyVault.vue");

  assert.match(manager, /当前调用 Key/);
  assert.match(manager, /Agent CLI 可以独立绑定任意一把/);
  assert.match(manager, /agentBindings\(option\)/);
  assert.match(manager, /<AgentCliIcon/);
  assert.doesNotMatch(manager, /主 Key|set-primary|isPrimary/);
});

test("pure API Key cards keep management local and never expose a fake refresh action", () => {
  const drawer = source("../src/components/provider-editor/ProviderApiKeyVault.vue");
  const managerState = source("../src/composables/useApiKeyManager.ts");
  const cardHeader = source("../src/components/provider-card/ProviderCardHeader.vue");
  const switcher = source("../src/components/provider-card/ProviderApiKeySwitcher.vue");
  const cardMenus = source("../src/components/provider-card/ProviderCardActionMenus.vue");

  assert.match(drawer, /v-if="remoteManaged"[\s\S]*同步站点/);
  assert.doesNotMatch(drawer, />刷新</);
  assert.doesNotMatch(managerState, /listLocalKeys/);
  assert.doesNotMatch(managerState, /void refreshApiKeyManager\(\)/);
  assert.match(managerState, /if \(!provider \|\| !apiKeyRemoteManaged\.value\) return;/);
  assert.match(managerState, /setDefaultKeyForProvider/);
  assert.match(cardHeader, /manageApiKeys/);
  assert.match(switcher, /defineEmits<\{/);
  assert.match(switcher, /IconSwap/);
  assert.match(switcher, /管理 API Key/);
  assert.match(cardHeader, /emit\('copyApiKey', provider\)/);
  assert.match(cardMenus, /props\.provider\.auth\.mode !== "apiKey"/);
  assert.match(cardMenus, /props\.provider\.actions\.apiKeyManagement \|\| hasManagedApiKeys\.value/);
  assert.match(drawer, /remoteManaged \? "站点与本地 Key" : "本地 Key"/);
  assert.match(drawer, /保存到当前卡片，不会在站点创建新令牌/);
  assert.match(drawer, /仅从当前卡片移除，不会撤销站点令牌/);
  assert.doesNotMatch(drawer, /纯 API Key.*同步站点/);
  assert.doesNotMatch(drawer, /currentTokenId|currentKey/);
});

test("account cards can open local API Key management without remote site capability", () => {
  const cardMenus = source("../src/components/provider-card/ProviderCardActionMenus.vue");

  assert.match(cardMenus, /const hasManagedApiKeys = computed/);
  assert.match(cardMenus, /props\.provider\.auth\.apiKeyOptions\.length > 0/);
  assert.match(cardMenus, /props\.provider\.auth\.mode !== "apiKey"/);
});

test("unsaved API Key mode never exposes account-only remote management actions", () => {
  const controller = source("../src/composables/useAppController.ts");
  const managerState = source("../src/composables/useApiKeyManager.ts");
  const rust = source("../src-tauri/src/models/provider/input.rs");

  assert.match(controller, /providerEditor\.draftProvider\.auth\.mode !== "apiKey"/);
  assert.match(managerState, /const remote = remoteKey && apiKeyRemoteManaged\.value/);
  assert.match(managerState, /站点上的令牌仍然有效/);
  assert.doesNotMatch(rust, /站点密钥请使用“删除站点密钥”/);
});

test("the editor keeps API Key selection and CRUD in one inline vault", () => {
  const vault = source("../src/components/provider-editor/ProviderApiKeyVault.vue");
  const credentials = source(
    "../src/components/provider-editor/ProviderEditorCredentialsSection.vue",
  );

  assert.match(vault, /设为当前/);
  assert.doesNotMatch(vault, /Key 额度|调用范围|使用记录/);
  assert.match(vault, /当前调用 Key/);
  assert.match(vault, /Agent CLI 可以独立绑定任意一把/);
  assert.match(credentials, /@set-default="emit\('set-default-managed-api-key', \$event\)"/);
});

test("closing API Key management invalidates pending work and releases the busy state", () => {
  const managerState = source("../src/composables/useApiKeyManager.ts");

  assert.match(managerState, /function closeApiKeyManager\(\)[\s\S]*requestRevision \+= 1;[\s\S]*apiKeyManagerOperation\.value = null;/);
  assert.match(managerState, /revision === requestRevision/);
  assert.match(managerState, /&& apiKeyManagerProvider\.value !== null/);
  assert.match(managerState, /options\.providers\.value\.find/);
});

test("deleting a bound Key is blocked and deleting the current Key selects a usable replacement", () => {
  const managerState = source("../src/composables/useApiKeyManager.ts");
  const rust = source("../src-tauri/src/services/provider_service/api_keys.rs");

  assert.match(managerState, /if \(boundAgents\.length > 0\)[\s\S]*Modal\.warning[\s\S]*return;/);
  assert.match(managerState, /当前调用 Key 会自动切换/);
  assert.match(rust, /selected_was_removed[\s\S]*cached\.iter\(\)\.find[\s\S]*auth\.api_key = replacement\.key\.clone\(\)/);
});

test("obsolete defaultCliKinds props are not kept as a second CLI state source", () => {
  for (const path of [
    "../src/components/ProviderBoard.vue",
    "../src/components/ProviderCard.vue",
    "../src/components/provider-card/ProviderCardActions.vue",
    "../src/components/provider-card/ProviderCardActionMenus.vue",
  ]) {
    assert.doesNotMatch(source(path), /defaultCliKinds|default-cli-kinds/);
  }
});
