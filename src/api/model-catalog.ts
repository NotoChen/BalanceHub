import { invoke } from "@tauri-apps/api/core";
export interface ModelPrice {
  input: number | null;
  output: number | null;
  cacheRead: number | null;
  cacheWrite: number | null;
}
export interface ModelMetadata {
  id: string;
  name: string;
  provider: string;
  contextWindow: number | null;
  outputLimit: number | null;
  reasoning: boolean | null;
  toolCall: boolean | null;
  inputModalities: string[];
  outputModalities: string[];
  price: ModelPrice;
}
export interface ModelMetadataSnapshot {
  models: ModelMetadata[];
  updatedAt: string | null;
  stale: boolean;
  error: string | null;
}
export function getModelMetadata(ids: string[], refresh = false) {
  return invoke<ModelMetadataSnapshot>("get_model_metadata", { ids, refresh });
}
