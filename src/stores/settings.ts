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
      // 后端每次快照都会重新序列化设置；内容没变时保留原引用。设置对象是
      // 卡片和外壳的 props，换身份会让整块界面在后台刷新时全部重渲染。
      if (JSON.stringify(settings) === JSON.stringify(this.settings)) return;
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
