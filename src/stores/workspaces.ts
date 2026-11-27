import { defineStore } from "pinia";
import {
  browseWorkspaceDirectories as browseWorkspaceDirectoriesCommand,
  forgetWorkspace as forgetWorkspaceCommand,
} from "../api/app";
import type {
  TemporaryCliPreference,
  Workspace,
} from "./provider-types";

export const useWorkspaceStore = defineStore("workspaces", {
  state: () => ({
    workspaces: [] as Workspace[],
    temporaryCliPreferences: [] as TemporaryCliPreference[],
  }),
  actions: {
    hydrate(workspaces: Workspace[], preferences: TemporaryCliPreference[]) {
      // 后台快照每次都会重建这两个数组；内容没变时保留引用，
      // 整个工作区外壳才不会跟着无关的中转站刷新一起重渲染。
      if (JSON.stringify(workspaces) !== JSON.stringify(this.workspaces)) {
        this.workspaces = workspaces;
      }
      if (JSON.stringify(preferences) !== JSON.stringify(this.temporaryCliPreferences)) {
        this.temporaryCliPreferences = preferences;
      }
    },
    recordLaunch(result: { workspaces: Workspace[]; preference?: TemporaryCliPreference | null }) {
      this.workspaces = result.workspaces;
      const preference = result.preference;
      if (!preference) return;
      this.temporaryCliPreferences = [
        ...this.temporaryCliPreferences.filter(
          (item) => item.providerId !== preference.providerId,
        ),
        preference,
      ];
    },
    removeProviderPreference(providerId: string) {
      this.temporaryCliPreferences = this.temporaryCliPreferences.filter(
        (preference) => preference.providerId !== providerId,
      );
    },
    async browse(path?: string) {
      return browseWorkspaceDirectoriesCommand(path);
    },
    async forget(path: string) {
      this.workspaces = await forgetWorkspaceCommand(path);
      this.temporaryCliPreferences = this.temporaryCliPreferences.map((preference) =>
        preference.workspacePath === path ? { ...preference, workspacePath: "" } : preference,
      );
      return this.workspaces;
    },
  },
});
