import { defineStore } from "pinia";
import { saveSettings as saveSettingsCommand } from "../api/app";
import { defaultSettings } from "./provider-defaults";
import type { AppSettings } from "./provider-types";

export const useSettingsStore = defineStore("settings", {
  state: () => ({
    settings: defaultSettings(),
  }),
  actions: {
    hydrate(settings: AppSettings) {
      this.settings = settings;
    },
    async save(settings: AppSettings) {
      this.settings = await saveSettingsCommand(settings);
      return this.settings;
    },
  },
});
