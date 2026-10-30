import { defineStore } from "pinia";
import type { OAuthImportPreview, OAuthLoginTask } from "./oauth-types";

export const useOAuthAccountStore = defineStore("oauth-accounts", {
  state: () => ({
    visible: false,
    view: "add" as "add" | "account" | "gateway" | "import" | "task",
    providerId: null as string | null,
    taskId: null as string | null,
    importedPreview: null as OAuthImportPreview | null,
    tasks: {} as Record<string, OAuthLoginTask>,
    eventError: "",
  }),
  actions: {
    accept(task: OAuthLoginTask) {
      const previous = this.tasks[task.id];
      if (!previous || previous.revision <= task.revision)
        this.tasks[task.id] = task;
    },
    openAdd() {
      this.view = "add";
      this.providerId = null;
      this.visible = true;
    },
    openAccount(id: string, gateway = false) {
      this.providerId = id;
      this.view = gateway ? "gateway" : "account";
      this.visible = true;
    },
    openTask(id: string) {
      this.taskId = id;
      this.view = "task";
      this.visible = true;
    },
    openImport(preview: OAuthImportPreview | null = null) {
      this.importedPreview = preview;
      this.view = "import";
      this.visible = true;
    },
    close() {
      this.visible = false;
    },
  },
});
