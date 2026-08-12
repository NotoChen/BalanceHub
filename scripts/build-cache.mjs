import { homedir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";

const home = homedir();

export function defaultTauriTargetDir() {
  if (process.platform === "darwin") {
    return join(home, "Library", "Caches", "BalanceHub", "cargo-target");
  }
  if (process.platform === "win32") {
    const localAppData = process.env.LOCALAPPDATA || join(home, "AppData", "Local");
    return join(localAppData, "BalanceHub", "cargo-target");
  }
  return join(process.env.XDG_CACHE_HOME || join(home, ".cache"), "balancehub", "cargo-target");
}

export function doctorTauriTargetDir() {
  return join(defaultTauriTargetDir(), "doctor");
}

export function effectiveTauriTargetDir() {
  const override = process.env.CARGO_TARGET_DIR?.trim();
  if (!override) return defaultTauriTargetDir();
  return isAbsolute(override) ? override : resolve(process.cwd(), override);
}
