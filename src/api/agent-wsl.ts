import { invoke } from "@tauri-apps/api/core";
import type { AgentCliKind } from "../stores/provider-types";

export interface WslAgent {
  agentKind: AgentCliKind;
  configRoot: string;
  executable: string | null;
  version: string | null;
}
export interface WslDistribution {
  id: string;
  name: string;
  running: boolean;
  home: string | null;
  agents: WslAgent[];
  networkMode: string | null;
  hostGateway: string | null;
  error: string | null;
}
export interface WslInventory {
  supported: boolean;
  distributions: WslDistribution[];
  error: string | null;
}
export const getWslInventory = (force = false) =>
  invoke<WslInventory>("get_wsl_inventory", { force });
export const startWslDistribution = (distroId: string) =>
  invoke<WslInventory>("start_wsl_distribution", { distroId });
