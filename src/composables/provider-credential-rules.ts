import type { ProviderAuthModeDescriptor, ProviderInput, ProviderProtocolDescriptor } from "../stores/providers";
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
    schema && missingCredentialRequirements(input, schema).length === 0,
  );
}

// Field paths and alternatives come from Rust's protocol descriptor. This
// helper only evaluates form completeness; backend authentication stays authoritative.
export function credentialFieldHasValue(input: ProviderInput, field: string) {
  let value: unknown = input.auth;
  for (const key of field.split(".")) {
    if (value === null || typeof value !== "object"
      || !Object.prototype.hasOwnProperty.call(value, key)) return false;
    value = Reflect.get(value, key);
  }
  return typeof value === "string" && Boolean(value.trim());
}

export function missingCredentialRequirements(input: ProviderInput, schema: ProviderAuthModeDescriptor) {
  const missing = schema.requiredFields
    .filter((field) => !credentialFieldHasValue(input, field))
    .map((field) => [field]);
  if (schema.requiredAnyFields.length > 0
    && !schema.requiredAnyFields.some((field) => credentialFieldHasValue(input, field))) {
    missing.push(schema.requiredAnyFields);
  }
  return missing;
}

export function canSkipAssistantAccessToken(input: ProviderInput, protocol: ProviderProtocolDescriptor) {
  return protocol.credentialAssistant.accessTokenSkipFields
    .some((field) => credentialFieldHasValue(input, field));
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
