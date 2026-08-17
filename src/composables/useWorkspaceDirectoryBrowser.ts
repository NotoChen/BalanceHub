import { ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { Workspace, WorkspaceDirectoryListing } from "../stores/providers";

interface UseWorkspaceDirectoryBrowserOptions {
  browse: (path?: string) => Promise<WorkspaceDirectoryListing>;
  forget: (path: string) => Promise<Workspace[]>;
}

export function useWorkspaceDirectoryBrowser(options: UseWorkspaceDirectoryBrowserOptions) {
  const workspaceDirectory = ref<WorkspaceDirectoryListing | null>(null);
  const workspacePathDraft = ref("");
  const workspaceBrowsing = ref(false);
  const workspaceForgettingPath = ref<string | null>(null);
  const workspaceBrowserError = ref("");
  let browseRequestId = 0;

  async function browseWorkspaceDirectory(path?: string) {
    const requestId = ++browseRequestId;
    workspaceBrowsing.value = true;
    workspaceBrowserError.value = "";
    try {
      const listing = await options.browse(path?.trim() || undefined);
      if (requestId !== browseRequestId) {
        return false;
      }
      workspaceDirectory.value = listing;
      workspacePathDraft.value = listing.currentPath;
      return true;
    } catch (error) {
      if (requestId === browseRequestId) {
        workspaceBrowserError.value = errorMessage(error);
      }
      return false;
    } finally {
      if (requestId === browseRequestId) {
        workspaceBrowsing.value = false;
      }
    }
  }

  async function forgetWorkspace(path: string) {
    if (workspaceForgettingPath.value) {
      return;
    }
    workspaceForgettingPath.value = path;
    try {
      await options.forget(path);
    } catch (error) {
      Message.error(errorMessage(error));
    } finally {
      workspaceForgettingPath.value = null;
    }
  }

  function resetWorkspaceDirectory() {
    invalidateWorkspaceDirectoryRequests();
    workspaceDirectory.value = null;
    workspacePathDraft.value = "";
    workspaceBrowserError.value = "";
  }

  function invalidateWorkspaceDirectoryRequests() {
    browseRequestId += 1;
    workspaceBrowsing.value = false;
  }

  return {
    workspaceDirectory,
    workspacePathDraft,
    workspaceBrowsing,
    workspaceForgettingPath,
    workspaceBrowserError,
    browseWorkspaceDirectory,
    forgetWorkspace,
    resetWorkspaceDirectory,
    invalidateWorkspaceDirectoryRequests,
  };
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
