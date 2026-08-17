import type { ProviderInput, ProviderProtocolDescriptor } from "../stores/providers";
import {
  providerAuthModeDescriptor,
  providerProtocolDescriptor,
} from "../utils/provider-protocol.ts";

interface CredentialResultStep {
  name: string;
  ok: boolean;
  message: string;
}

export function canRunCredentialAssistantForInput(
  input: ProviderInput,
  descriptors: ProviderProtocolDescriptor[],
  busy: boolean,
) {
  if (busy || input.auth.mode === "apiKey") {
    return false;
  }
  if (!input.identity.baseUrl.trim()) {
    return false;
  }
  const protocol = providerProtocolDescriptor(descriptors, input.identity.protocol);
  if (!protocol?.credentialAssistant.enabled) {
    return false;
  }
  const schema = providerAuthModeDescriptor(
    descriptors,
    input.identity.protocol,
    input.auth.mode,
  );
  return Boolean(
    schema && schema.requiredFields.every((field) => credentialFieldHasValue(input, field)),
  );
}

function credentialFieldHasValue(input: ProviderInput, field: string) {
  const value = input.auth[field as keyof ProviderInput["auth"]];
  return typeof value === "string" && Boolean(value.trim());
}

export function blockingCredentialCompletionFailures(steps: CredentialResultStep[]) {
  return steps.filter((step) => {
    if (step.ok) {
      return false;
    }
    return !(
      step.name.includes("访问令牌") ||
      step.name.includes("API 密钥") ||
      step.name.includes("API Key")
    );
  });
}

export function isEmptyApiKeyMessage(message: string) {
  return /站点没有(已有|可用)?\s*API\s*Key|没有已有\s*API\s*Key/i.test(message);
}
