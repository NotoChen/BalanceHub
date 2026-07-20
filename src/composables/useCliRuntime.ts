import { computed, ref, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type {
  CliRuntimeSnapshot,
  Provider,
  TemporaryCliInstance,
} from "../stores/providers";

interface UseCliRuntimeOptions {
  providers: Ref<Provider[]>;
  cliRuntime: Ref<CliRuntimeSnapshot>;
  refresh: () => Promise<CliRuntimeSnapshot>;
  activate: (instanceId: string) => Promise<void>;
  relaunch: (instanceId: string) => Promise<TemporaryCliInstance>;
}

export function useCliRuntime(options: UseCliRuntimeOptions) {
  const cliInstancesVisible = ref(false);
  const cliInstancesProviderId = ref<string | null>(null);
  const activatingCliInstanceId = ref<string | null>(null);
  const relaunchingCliInstanceId = ref<string | null>(null);

  const cliInstancesProvider = computed(() =>
    options.providers.value.find(
      (provider) => provider.identity.id === cliInstancesProviderId.value,
    ) ?? null,
  );

  const providerCliInstances = computed(() =>
    options.cliRuntime.value.instances.filter(
      (instance) => instance.providerId === cliInstancesProviderId.value,
    ),
  );

  function openCliInstances(provider: Provider) {
    cliInstancesProviderId.value = provider.identity.id;
    cliInstancesVisible.value = true;
    void refreshCliRuntime();
  }

  async function refreshCliRuntime() {
    try {
      await options.refresh();
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    }
  }

  async function relaunchCliInstance(instance: TemporaryCliInstance) {
    relaunchingCliInstanceId.value = instance.id;
    try {
      await options.relaunch(instance.id);
      Message.success(`已重新启动 ${instance.cliKind === "codex" ? "Codex" : "Claude Code"}`);
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      relaunchingCliInstanceId.value = null;
    }
  }

  async function activateCliInstance(instance: TemporaryCliInstance) {
    activatingCliInstanceId.value = instance.id;
    try {
      await options.activate(instance.id);
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      activatingCliInstanceId.value = null;
    }
  }

  return {
    cliInstancesVisible,
    cliInstancesProvider,
    providerCliInstances,
    activatingCliInstanceId,
    relaunchingCliInstanceId,
    openCliInstances,
    refreshCliRuntime,
    activateCliInstance,
    relaunchCliInstance,
  };
}
