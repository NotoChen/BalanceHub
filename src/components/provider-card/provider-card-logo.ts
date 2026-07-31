import type { Provider } from "../../stores/providers";
import newApiLogo from "../../assets/logos/new-api.png";
import sub2ApiLogo from "../../assets/logos/sub2api.svg";

export function providerLogoSrc(provider: Provider) {
  if (provider.identity.siteLogo?.trim()) {
    return provider.identity.siteLogo;
  }
  return provider.identity.protocol === "sub2Api" ? sub2ApiLogo : newApiLogo;
}
export function applyProviderLogoFallback(event: Event, provider: Provider) {
  const image = event.target as HTMLImageElement;
  const fallback = provider.identity.protocol === "sub2Api" ? sub2ApiLogo : newApiLogo;
  if (image.src !== fallback) {
    image.src = fallback;
  }
}
