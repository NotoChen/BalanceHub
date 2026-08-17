import type {
  AuthMode,
  ProviderProtocol,
  ProviderProtocolDescriptor,
} from "../stores/providers";

export function providerProtocolDescriptor(
  descriptors: ProviderProtocolDescriptor[],
  kind: ProviderProtocol,
) {
  return descriptors.find((descriptor) => descriptor.kind === kind);
}

export function providerProtocolLabel(
  descriptors: ProviderProtocolDescriptor[],
  kind: ProviderProtocol,
) {
  return providerProtocolDescriptor(descriptors, kind)?.label ?? kind;
}

export function providerAuthModeLabel(
  descriptors: ProviderProtocolDescriptor[],
  mode: AuthMode,
) {
  for (const descriptor of descriptors) {
    const authMode = descriptor.authModes.find((candidate) => candidate.mode === mode);
    if (authMode) return authMode.label;
  }
  return mode;
}

export function providerAuthModeDescriptor(
  descriptors: ProviderProtocolDescriptor[],
  protocol: ProviderProtocol,
  mode: AuthMode,
) {
  return providerProtocolDescriptor(descriptors, protocol)?.authModes.find(
    (candidate) => candidate.mode === mode,
  );
}
