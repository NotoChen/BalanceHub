import assert from "node:assert/strict";
import test from "node:test";
import * as vue from "vue";
import { effectScope, nextTick, ref } from "vue";
import { normalizeLivenessTiming } from "../src/utils/liveness-defaults.ts";
import { createSettingsSaveQueue } from "../src/utils/settings-save-queue.ts";
import { loadSource } from "./helpers/load-source.ts";

/** 只保留本测试读取的字段；控制器按默认值浅拷贝并整体替换表单。 */
interface Settings {
  themeMode: string;
  launchAtLogin: boolean;
  livenessModel: string;
}

function defaultSettings(): Settings {
  return { themeMode: "system", launchAtLogin: false, livenessModel: "" };
}

interface SettingsController {
  settingsForm: Settings;
  requestExternalLaunchSync: () => void;
  syncFromSettings: (value?: Settings) => boolean;
}

function fixture(t: { after: (callback: () => void) => void }) {
  const themeCalls: string[] = [];
  const loginChanges: boolean[] = [];
  const errors: string[] = [];
  const settings = ref<Settings>(defaultSettings());
  const { useSettingsController } = loadSource<{
    useSettingsController: (options: Record<string, unknown>) => SettingsController;
  }>("composables/useSettingsController.ts", {
    vue: { ...vue, onBeforeUnmount: vue.onScopeDispose },
    "@tauri-apps/plugin-autostart": {
      isEnabled: async () => false,
      enable: async () => {
        loginChanges.push(true);
      },
      disable: async () => {
        loginChanges.push(false);
      },
    },
    "../api/transport.ts": { isWebHost: false },
    "@arco-design/web-vue": {
      Message: { error: (message: string) => errors.push(message) },
    },
    "../stores/providers": { defaultSettings },
    "../utils/liveness-defaults": { normalizeLivenessTiming },
    "./useThemeMode": {
      useThemeMode: () => ({
        applyTheme: (mode: string) => themeCalls.push(mode),
        setupThemeListener: () => {},
        cleanupThemeListener: () => {},
      }),
    },
    "../utils/provider-display": {
      providerDisplayLabel: (provider: { identity: { name: string } }) =>
        provider.identity.name,
    },
    "../utils/settings-save-queue": {
      // 保存延迟放大到测试结束之后，避免定时器把过期草稿写回表单；
      // 这里要验证的是 acceptExternal / dirty 判定，不是防抖本身。
      createSettingsSaveQueue: (
        options: Parameters<typeof createSettingsSaveQueue>[0],
      ) => createSettingsSaveQueue({ ...options, delayMs: 60_000 }),
    },
  });
  const scope = effectScope();
  const controller = scope.run(() =>
    useSettingsController({
      providers: ref([]),
      settings,
      initialSettings: defaultSettings(),
      saveSettings: async (value: Settings) => value,
    }),
  )!;
  t.after(() => scope.stop());
  return { controller, settings, themeCalls, loginChanges, errors };
}

async function flush() {
  await Promise.resolve();
  await Promise.resolve();
}

test("a backend settings snapshot reaches the form and the system login item", async (t) => {
  const context = fixture(t);
  context.controller.requestExternalLaunchSync();
  context.settings.value = {
    ...defaultSettings(),
    themeMode: "dark",
    launchAtLogin: true,
  };
  await nextTick();
  assert.equal(context.controller.settingsForm.themeMode, "dark");
  assert.equal(context.themeCalls.at(-1), "dark");
  await flush();
  assert.deepEqual(context.loginChanges, [true]);
});

test("an unsaved draft is not overwritten and can sync again once it is clean", async (t) => {
  const context = fixture(t);
  context.controller.settingsForm.themeMode = "dark";
  await nextTick();
  context.controller.requestExternalLaunchSync();
  context.settings.value = {
    ...defaultSettings(),
    themeMode: "light",
    launchAtLogin: true,
  };
  await nextTick();
  assert.equal(context.controller.settingsForm.themeMode, "dark");
  await flush();
  // 草稿还在，外部配置的开机启动同步随草稿一起作废。
  assert.deepEqual(context.loginChanges, []);

  // 用户撤销草稿后，下一份后端快照应当重新水合。
  context.controller.settingsForm.themeMode = "system";
  await nextTick();
  context.settings.value = {
    ...defaultSettings(),
    livenessModel: "fixture-model",
  };
  await nextTick();
  assert.equal(context.controller.settingsForm.livenessModel, "fixture-model");
  assert.equal(context.controller.settingsForm.themeMode, "system");
  assert.deepEqual(context.errors, []);
});
