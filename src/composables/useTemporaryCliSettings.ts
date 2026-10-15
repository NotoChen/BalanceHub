import { computed, onScopeDispose, ref, watch, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import * as api from "../api/temporary-cli-settings";
import { withTimeout } from "../utils/promise-timeout";
import {
  copyTemporaryCliRunSettings,
  defaultTemporaryCliRunSettings,
  sameTemporaryCliRunSettings,
  type TemporaryCliAssetOption,
  type TemporaryCliProfile,
  type TemporaryCliRunSettings,
  type TemporaryCliSettingsCapabilities,
  type TemporaryCliSettingsRequest,
} from "../stores/temporary-cli-settings-types";

export function useTemporaryCliSettings(
  active: Ref<boolean>,
  request: Ref<TemporaryCliSettingsRequest>,
  model: Ref<TemporaryCliRunSettings>,
) {
  const capabilities = ref<TemporaryCliSettingsCapabilities | null>(null);
  const loading = ref(false);
  const error = ref("");
  const assets = ref<TemporaryCliAssetOption[]>([]);
  const assetLoading = ref(false);
  const assetError = ref("");
  const assetNotice = ref("");
  const assetsLoaded = ref(false);
  const assetsExpanded = ref(false);
  const profiles = ref<TemporaryCliProfile[]>([]);
  const profileId = ref("");
  const profileName = ref("");
  const saving = ref(false);
  const profileError = ref("");
  const profileDrafts = new Map<
    TemporaryCliSettingsRequest["cliKind"],
    { id: string; name: string }
  >();
  let requestId = 0;
  let assetRequestId = 0;
  let profileRequestId = 0;
  let contextRevision = 0;
  let mutationId = 0;
  let disposed = false;
  let selectionRevision = 0;
  let capabilityTimer: ReturnType<typeof setTimeout> | undefined;
  const matchingProfiles = computed(() =>
    profiles.value.filter(
      (profile) => profile.cliKind === request.value.cliKind,
    ),
  );
  const selectedProfile = computed(() =>
    matchingProfiles.value.find((profile) => profile.id === profileId.value),
  );
  const profileSettings = computed(() => ({
    ...copyTemporaryCliRunSettings(model.value),
    keepSession:
      request.value.sessionMode === "history" || model.value.keepSession,
  }));
  const modified = computed(
    () =>
      !sameTemporaryCliRunSettings(
        profileSettings.value,
        defaultTemporaryCliRunSettings(),
      ),
  );
  const profileModified = computed(() => {
    const profile = selectedProfile.value;
    return Boolean(
      profile &&
      (profileName.value.trim() !== profile.name ||
        !sameTemporaryCliRunSettings(profileSettings.value, profile.settings)),
    );
  });
  const canSaveProfile = computed(
    () =>
      Boolean(profileName.value.trim()) &&
      (!selectedProfile.value || profileModified.value),
  );
  const profileSaveLabel = computed(() => {
    if (!profileId.value) return "保存";
    return selectedProfile.value && !profileModified.value
      ? "已保存"
      : "更新方案";
  });

  async function loadCapabilities(blockControls = true) {
    const id = ++requestId;
    if (!active.value || !request.value.cliPath) {
      loading.value = false;
      return;
    }
    loading.value = blockControls;
    error.value = "";
    try {
      const result = await withTimeout(
        api.getTemporaryCliSettings({ ...request.value }),
        8_000,
        "读取启动选项超时，可重试或使用默认设置",
      );
      if (id === requestId && !disposed) capabilities.value = result;
    } catch (cause) {
      if (id === requestId && !disposed) error.value = String(cause);
    } finally {
      if (id === requestId && !disposed) loading.value = false;
    }
  }

  async function loadAssets(refresh = false) {
    const id = ++assetRequestId;
    if (!active.value || !request.value.workdir || !request.value.cliPath) {
      assetLoading.value = false;
      return;
    }
    assetLoading.value = true;
    assetError.value = "";
    try {
      const result = await withTimeout(
        api.getTemporaryCliAssets({ ...request.value }, refresh),
        20_000,
        "读取当前 Agent 的资产超时，请重试",
      );
      if (id === assetRequestId && !disposed) {
        assets.value = result.assets;
        assetNotice.value = result.notice;
        assetsLoaded.value = true;
      }
    } catch (cause) {
      if (id === assetRequestId && !disposed) assetError.value = String(cause);
    } finally {
      if (id === assetRequestId && !disposed) assetLoading.value = false;
    }
  }

  async function loadProfiles() {
    const id = ++profileRequestId;
    try {
      const result = await withTimeout(
        api.listTemporaryCliProfiles(),
        8_000,
        "读取启动方案超时",
      );
      if (id === profileRequestId && !disposed) profiles.value = result;
    } catch (cause) {
      if (id === profileRequestId && !disposed)
        profileError.value = String(cause);
    }
  }

  function selectProfile(id: string) {
    selectionRevision++;
    profileId.value = id;
    const profile = matchingProfiles.value.find((item) => item.id === id);
    model.value = profile
      ? copyTemporaryCliRunSettings(profile.settings)
      : defaultTemporaryCliRunSettings();
    profileName.value = profile?.name ?? "";
    if (
      model.value.disabledAssets.length &&
      assetsExpanded.value &&
      !assetsLoaded.value &&
      !assetLoading.value
    ) {
      void loadAssets();
    }
  }

  async function saveProfile(asNew = false) {
    if (
      saving.value ||
      !profileName.value.trim() ||
      (!asNew && !canSaveProfile.value)
    )
      return;
    saving.value = true;
    profileError.value = "";
    const revision = contextRevision;
    const selection = selectionRevision;
    const id = ++mutationId;
    profileRequestId++;
    const profile = {
      id: asNew ? "" : profileId.value,
      name: profileName.value.trim(),
      cliKind: request.value.cliKind,
      settings: profileSettings.value,
    };
    try {
      const result = await withTimeout(
        api.saveTemporaryCliProfile(profile),
        8_000,
        "保存启动方案超时，请重新读取方案确认结果",
      );
      if (disposed || revision !== contextRevision || id !== mutationId) return;
      profiles.value = result;
      if (selection !== selectionRevision) return;
      profileId.value =
        profile.id ||
        [...result]
          .reverse()
          .find(
            (item) =>
              item.cliKind === profile.cliKind && item.name === profile.name,
          )?.id ||
        "";
      Message.success("启动方案已保存");
    } catch (cause) {
      if (!disposed && revision === contextRevision && id === mutationId)
        profileError.value = String(cause);
    } finally {
      if (!disposed && id === mutationId) saving.value = false;
    }
  }

  async function deleteProfile() {
    if (saving.value || !profileId.value) return;
    saving.value = true;
    profileError.value = "";
    const revision = contextRevision;
    const selection = selectionRevision;
    const operation = ++mutationId;
    profileRequestId++;
    const id = profileId.value;
    try {
      const result = await withTimeout(
        api.deleteTemporaryCliProfile(id),
        8_000,
        "删除启动方案超时，请重新读取方案确认结果",
      );
      if (disposed || revision !== contextRevision || operation !== mutationId)
        return;
      profiles.value = result;
      if (selection === selectionRevision) {
        profileId.value = "";
        profileName.value = "";
      }
      Message.success("启动方案已删除，本次设置仍然保留");
    } catch (cause) {
      if (!disposed && revision === contextRevision && operation === mutationId)
        profileError.value = String(cause);
    } finally {
      if (!disposed && operation === mutationId) saving.value = false;
    }
  }

  watch(
    [
      () => active.value,
      () => request.value.cliKind,
      () => request.value.cliPath,
    ] as const,
    ([isActive, kind], previous) => {
      if (isActive && previous?.[0] && previous[1]) {
        profileDrafts.set(previous[1], {
          id: profileId.value,
          name: profileName.value,
        });
      }
      if (!isActive) profileDrafts.clear();
      const draft = isActive ? profileDrafts.get(kind) : undefined;
      contextRevision++;
      profileRequestId++;
      mutationId++;
      saving.value = false;
      profileError.value = "";
      profileId.value = draft?.id ?? "";
      profileName.value = draft?.name ?? "";
      if (active.value) void loadProfiles();
    },
    { immediate: true },
  );
  watch(
    [
      () => active.value,
      () => request.value.cliKind,
      () => request.value.cliPath,
      () => request.value.workdir,
      () => request.value.model,
      () => request.value.sessionMode,
      () => request.value.inheritedModel,
      () => request.value.nativeOptions?.contextWindow,
      () => request.value.nativeOptions?.autoCompact,
      () => request.value.nativeOptions?.modelMappings,
      () => request.value.nativeOptions?.subagentModel,
      () => request.value.nativeOptions?.subagents,
    ] as const,
    (current, previous) => {
      requestId++;
      // Changing the model refreshes the fields in place. Only switching the
      // Agent or directory should discard this panel's configuration context.
      const sameContext =
        current[0] &&
        previous?.[0] &&
        current[1] === previous[1] &&
        current[2] === previous[2] &&
        current[3] === previous[3];
      const valuesOnly =
        sameContext &&
        current.slice(0, 7).every((value, index) => value === previous[index]);
      if (!sameContext) capabilities.value = null;
      loading.value = false;
      error.value = "";
      clearTimeout(capabilityTimer);
      if (active.value && request.value.cliPath) {
        loading.value = !valuesOnly;
        capabilityTimer = setTimeout(
          () => void loadCapabilities(!valuesOnly),
          180,
        );
      }
    },
    { immediate: true },
  );
  watch(
    [
      () => active.value,
      () => request.value.cliKind,
      () => request.value.cliPath,
      () => request.value.workdir,
    ],
    () => {
      assetRequestId += 1;
      assets.value = [];
      assetError.value = "";
      assetNotice.value = "";
      assetsLoaded.value = false;
      assetLoading.value = false;
      if (assetsExpanded.value) void loadAssets();
    },
  );
  watch(assetsExpanded, (expanded) => {
    if (expanded && !assetsLoaded.value && !assetLoading.value)
      void loadAssets();
  });
  onScopeDispose(() => {
    disposed = true;
    clearTimeout(capabilityTimer);
    requestId++;
    assetRequestId++;
    profileRequestId++;
  });
  return {
    capabilities,
    loading,
    error,
    loadCapabilities,
    assets,
    assetLoading,
    assetError,
    assetNotice,
    assetsLoaded,
    assetsExpanded,
    loadAssets,
    matchingProfiles,
    modified,
    profileModified,
    canSaveProfile,
    profileSaveLabel,
    profileId,
    profileName,
    saving,
    profileError,
    selectProfile,
    saveProfile,
    deleteProfile,
  };
}
