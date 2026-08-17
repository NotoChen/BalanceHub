import type { Ref } from "vue";
import type {
  Provider,
  ProviderApiKeyOption,
  ProviderInput,
  ProviderProtocolDetectionResult,
  ProviderProtocolDescriptor,
  ProviderSiteProbeResult,
} from "../stores/providers";
import type { ProtocolSelectionSource } from "./provider-editor-shared";

export type CredentialCompletionState =
  | "idle"
  | "probingSite"
  | "resolvingCredentials"
  | "needAccessTokenConfirm"
  | "generatingAccessToken"
  | "needApiKeySelection"
  | "needApiKeyName"
  | "creatingApiKey"
  | "saving"
  | "done"
  | "failed";

export interface CredentialCompletionStep {
  key: string;
  name: string;
  status: "pending" | "running" | "done" | "error" | "skipped";
  message: string;
}

export interface UseProviderCredentialCompletionOptions {
  draftProvider: ProviderInput;
  providerProtocols: () => ProviderProtocolDescriptor[];
  drawerVisible: Ref<boolean>;
  editorSession: Ref<number>;
  editingProviderId: Ref<string | null>;
  probingSite: Ref<boolean>;
  siteProbeResult: Ref<ProviderSiteProbeResult | null>;
  protocolDetectionResult: Ref<ProviderProtocolDetectionResult | null>;
  protocolSelectionSource: Ref<ProtocolSelectionSource>;
  protocolSelectionBaseUrl: Ref<string>;
  completingCredentials: Ref<boolean>;
  credentialCompletionMessage: Ref<string>;
  credentialCompletionSteps: Ref<{ name: string; ok: boolean; message: string }[]>;
  siteNameSourceBaseUrl: Ref<string>;
  detectProviderProtocol: (input: ProviderInput) => Promise<ProviderProtocolDetectionResult>;
  probeProviderSite: (input: ProviderInput) => Promise<ProviderSiteProbeResult>;
  completeProviderCredentials: (input: ProviderInput) => Promise<{
    input: ProviderInput;
    changedFields: string[];
    steps: { name: string; ok: boolean; message: string }[];
    apiKeyOptions: ProviderApiKeyOption[];
  }>;
  createApiKeyForInput: (input: ProviderInput, name: string) => Promise<ProviderApiKeyOption>;
  generateAccessTokenForInput: (input: ProviderInput) => Promise<string>;
  setApiKeyOptions: (options: ProviderApiKeyOption[]) => void;
  saveDraftAndFindProvider: (isCurrent?: () => boolean) => Promise<Provider | undefined>;
  refreshAfterSave: (provider: Provider | undefined) => void;
}

export interface EditorRequestContext {
  editorSession: number;
  providerId: string | null;
  inputFingerprint: string;
}

export interface ProviderCredentialRequestGuard {
  snapshotInput: () => ProviderInput;
  captureRequestContext: (input?: ProviderInput) => EditorRequestContext;
  editorSessionIsActive: (context: EditorRequestContext) => boolean;
  editorSessionIsCurrent: (context: EditorRequestContext) => boolean;
  requestContextIsCurrent: (context: EditorRequestContext) => boolean;
}

export interface CompletionRunOptions {
  notify?: boolean;
  save?: boolean;
}

export interface ProviderSiteProbeOptions {
  silent?: boolean;
  force?: boolean;
  skipDetection?: boolean;
}

export type ProviderSiteProbe = (
  options?: ProviderSiteProbeOptions,
) => Promise<ProviderSiteProbeResult | null | undefined>;
