import { defineStore } from "pinia";
import {
  browseWorkspaceDirectories as browseWorkspaceDirectoriesCommand,
  forgetWorkspace as forgetWorkspaceCommand,
} from "../api/app";
import type {
  TemporaryCliLaunchResult,
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
      this.workspaces = workspaces;
      this.temporaryCliPreferences = preferences;
    },
    recordLaunch(result: TemporaryCliLaunchResult) {
      this.workspaces = result.workspaces;
      this.temporaryCliPreferences = [
        ...this.temporaryCliPreferences.filter(
          (preference) => preference.providerId !== result.preference.providerId,
        ),
        result.preference,
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
