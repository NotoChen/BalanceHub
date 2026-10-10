import assert from "node:assert/strict";
import test from "node:test";
import { createRenderer, defineComponent, nextTick, ref } from "vue";
import { createServer } from "vite";
import { createSettingsSaveQueue } from "../src/utils/settings-save-queue.ts";
import type { AppSettings } from "../src/stores/provider-types.ts";

test("a cloud update reaches an open clean settings window while pending edits stay protected", async (t) => {
  // Startup stays pending: this test exercises settings hydration only, without
  // native listeners, discovery, autostart or any Agent configuration writes.
  const server = await createServer({
    configFile: false,
    appType: "custom",
    logLevel: "silent",
    server: { middlewareMode: true, hmr: false },
    optimizeDeps: { noDiscovery: true, include: [] },
    plugins: [
      {
        name: "cloud-settings-view-fixture",
        resolveId(id, importer) {
          if (id === "../api/app" && importer?.endsWith("/useAppLifecycle.ts"))
            return "\0cloud-settings-app";
        },
        load(id) {
          if (id === "\0cloud-settings-app")
            return "export async function hostPlatform() { return null; }";
        },
      },
    ],
  });
  t.after(() => server.close());
  const { useAppLifecycle } = (await server.ssrLoadModule(
    "/src/composables/useAppLifecycle.ts",
  )) as typeof import("../src/composables/useAppLifecycle.ts");
  const settings = ref({ themeMode: "light" } as AppSettings);
  let draft = { themeMode: "light" } as AppSettings;
  let resolveSave!: (value: AppSettings) => void;
  const queue = createSettingsSaveQueue({
    read: () => ({ ...draft }),
    write: () =>
      new Promise<AppSettings>((resolve) => {
        resolveSave = resolve;
      }),
    accept: (value) => {
      draft = value;
    },
    state: () => {},
    failed: (message) => assert.fail(message),
  });
  t.after(queue.dispose);
  const renderer = createRenderer<
    Record<string, unknown>,
    Record<string, unknown>
  >({
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
  const app = renderer.createApp(
    defineComponent({
      setup() {
        useAppLifecycle({
          settings,
          settingsForm: draft,
          settingsDrawerVisible: ref(true),
          loadError: ref(null),
          initialize: () => new Promise(() => {}),
          syncFromSettings: (value = settings.value) => {
            if (queue.acceptExternal(value)) draft = { ...value };
          },
          setupThemeListener() {},
          cleanupThemeListener() {},
          syncLaunchAtLogin: async () => {},
          autoProbeCliTools: async () => {},
          reloadProviders() {},
          applyTheme() {},
          flushSettingsSave: queue.flush,
          resetProviderPointerDrag() {},
        });
        return () => null;
      },
    }),
  );
  app.mount({});
  t.after(() => app.unmount());
  settings.value = { themeMode: "dark" } as AppSettings;
  await nextTick();
  assert.equal(
    draft.themeMode,
    "dark",
    "an open clean window receives the cloud update",
  );
  draft.themeMode = "system";
  const saving = queue.flush();
  await Promise.resolve();
  settings.value = { themeMode: "light" } as AppSettings;
  await nextTick();
  assert.equal(
    draft.themeMode,
    "system",
    "pending local edits retain ownership of the draft",
  );
  resolveSave({ themeMode: "system" } as AppSettings);
  await saving;
});
