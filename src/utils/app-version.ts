export function formatAppVersionLabel(value: string | null | undefined) {
  const version = typeof value === "string" ? value.trim() : "";
  return version ? `v${version}` : "开发环境";
}
