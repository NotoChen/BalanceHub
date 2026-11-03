//! 宿主能力：浏览器版通过服务端拿到已实现的命令清单，桌面版默认全部可用。
//!
//! 结果按进程缓存一次；界面首次加载后据此隐藏桌面专属入口。

import { computed, readonly, shallowRef } from "vue";
import { invoke, isWebHost } from "../api/transport.ts";

export interface HostCapabilities {
  host: string;
  /** 浏览器版才有；桌面版为 null，表示不限制。 */
  commands: Set<string> | null;
}

const capabilities = shallowRef<HostCapabilities | null>(null);
let pending: Promise<HostCapabilities> | null = null;

export function loadHostCapabilities(): Promise<HostCapabilities> {
  if (capabilities.value) return Promise.resolve(capabilities.value);
  if (pending) return pending;
  pending = (isWebHost
    ? invoke<{ host: string; commands: string[] }>("get_host_capabilities")
    : Promise.resolve({ host: "desktop", commands: [] as string[] }))
    .then((value) => {
      capabilities.value = {
        host: value.host,
        commands: isWebHost ? new Set(value.commands) : null,
      };
      return capabilities.value;
    })
    .catch(() => {
      capabilities.value = {
        host: isWebHost ? "server" : "desktop",
        commands: new Set(),
      };
      return capabilities.value;
    })
    .finally(() => {
      pending = null;
    });
  return pending;
}

/** 当前宿主是否实现了某个命令；能力清单未就绪时按可用处理，避免首帧闪烁。 */
export function supportsCommand(command: string): boolean {
  const commands = capabilities.value?.commands;
  return commands === null || commands === undefined || commands.has(command);
}

export function useHostCapabilities() {
  /// Agent 能力由命令清单决定：后端机器上有 CLI 工具时同样可用。
  const agentAvailable = computed(() => supportsCommand("get_agent_runtime_snapshot"));
  const commandAvailable = (command: string) => supportsCommand(command);
  const resolved = computed(() => capabilities.value !== null);
  return {
    capabilities: readonly(capabilities),
    agentAvailable,
    commandAvailable,
    resolved,
    ensure: loadHostCapabilities,
  };
}
