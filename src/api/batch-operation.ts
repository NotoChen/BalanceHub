import { invoke, type Channel } from "@tauri-apps/api/core";
import type { Provider } from "../stores/providers";

export type ProviderBatchOperation = "refresh" | "checkIn";
export type ProviderBatchStatus = "pending" | "running" | "success" | "failed" | "skipped";

export interface ProviderBatchDetails {
  username: string;
  userId: string;
  available: number;
  used: number;
  known: boolean;
  totalKnown: boolean;
  quotaDisplayType: string;
  currencySymbol: string;
  unlimited: boolean;
  modelCount: number;
  lastSyncedAt?: string | null;
  lastCheckedInAt?: string | null;
  lastCheckInUser: string;
  quotaDelta?: number | null;
}

export interface ProviderBatchProgressItem {
  providerId: string;
  name: string;
  baseUrl: string;
  status: ProviderBatchStatus;
  message: string;
  details?: ProviderBatchDetails | null;
}

export interface ProviderBatchSummary {
  total: number;
  completed: number;
  success: number;
  failed: number;
  skipped: number;
}

export type ProviderBatchProgressEvent =
  | {
      event: "started";
      data: {
        operation: ProviderBatchOperation;
        total: number;
        items: ProviderBatchProgressItem[];
      };
    }
  | {
      event: "providerStarted";
      data: {
        operation: ProviderBatchOperation;
        item: ProviderBatchProgressItem;
      };
    }
  | {
      event: "providerFinished";
      data: {
        operation: ProviderBatchOperation;
        item: ProviderBatchProgressItem;
      };
    }
  | {
      event: "completed";
      data: {
        operation: ProviderBatchOperation;
        summary: ProviderBatchSummary;
      };
    };

export interface BatchOperationResult {
  updatedProviders: Provider[];
}

export function refreshAllProvidersWithProgress(onEvent: Channel<ProviderBatchProgressEvent>) {
  return invoke<BatchOperationResult>("refresh_all_providers_with_progress", { onEvent });
}

export function checkInAllProviders(onEvent: Channel<ProviderBatchProgressEvent>) {
  return invoke<BatchOperationResult>("check_in_all_providers", { onEvent });
}
