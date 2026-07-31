import type { ProviderInput } from "../stores/providers";

interface CredentialResultStep {
  name: string;
  ok: boolean;
  message: string;
}

export function canRunCredentialAssistantForInput(input: ProviderInput, busy: boolean) {
  if (busy || input.auth.mode === "apiKey" || input.identity.protocol === "api") {
    return false;
  }
  if (!input.identity.baseUrl.trim()) {
    return false;
  }
  if (input.identity.protocol === "sub2Api") {
    if (input.auth.mode === "password") {
      return Boolean(input.auth.loginUsername.trim() && input.auth.loginPassword.trim());
    }
    return Boolean(input.auth.accessToken.trim());
  }
  if (input.auth.mode === "session") {
    return Boolean(input.auth.sessionCookie.trim());
  }
  if (input.auth.mode === "password") {
    return Boolean(input.auth.loginUsername.trim() && input.auth.loginPassword.trim());
  }
  return Boolean(input.auth.accessToken.trim() && input.auth.apiUser.trim());
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
