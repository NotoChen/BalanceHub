import { defineStore } from "pinia";
import { saveSettings as saveSettingsCommand } from "../api/app";
import { defaultSettings } from "./provider-defaults";
import type { AppSettings } from "./provider-types";

export const useSettingsStore = defineStore("settings", {
  state: () => ({
    settings: defaultSettings(),
    saveRequestId: 0,
  }),
  actions: {
    hydrate(settings: AppSettings) {
      this.saveRequestId += 1;
      this.settings = settings;
    },
    async save(settings: AppSettings, expected: AppSettings) {
      const requestId = ++this.saveRequestId;
      const saved = await saveSettingsCommand(settings, expected);
      if (requestId === this.saveRequestId) this.settings = saved;
      return saved;
    },
  },
});
