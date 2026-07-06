import type { AppSettings, ThemeMode } from "../stores/providers";

export function useThemeMode(settings: Pick<AppSettings, "themeMode">) {
  let themeMediaQuery: MediaQueryList | null = null;
  let themeMediaListener: ((event: MediaQueryListEvent) => void) | null = null;

  function resolveThemeMode(mode: ThemeMode) {
    if (mode === "system") {
      return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
    }
    return mode;
  }

  function applyTheme(mode: ThemeMode) {
    const resolvedMode = resolveThemeMode(mode);
    const isDark = resolvedMode === "dark";
    document.documentElement.classList.toggle("theme-dark", isDark);
    document.documentElement.classList.toggle("theme-light", !isDark);
    document.body.setAttribute("arco-theme", isDark ? "dark" : "light");
  }

  function setupThemeListener() {
    themeMediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    themeMediaListener = () => {
      if (settings.themeMode === "system") {
        applyTheme("system");
      }
    };
    themeMediaQuery.addEventListener("change", themeMediaListener);
  }

  function cleanupThemeListener() {
    if (themeMediaQuery && themeMediaListener) {
      themeMediaQuery.removeEventListener("change", themeMediaListener);
    }
    themeMediaQuery = null;
    themeMediaListener = null;
  }

  return {
    applyTheme,
    setupThemeListener,
    cleanupThemeListener,
  };
}
