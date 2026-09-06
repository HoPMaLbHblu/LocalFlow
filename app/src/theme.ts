// Light / dark / system theme. The CSS reads `data-theme` on <html>;
// without it, the page follows the Windows setting.

import { useEffect, useState } from "react";

export type Theme = "system" | "light" | "dark";

const CACHE_KEY = "localflow.theme";
const DARK_QUERY = "(prefers-color-scheme: dark)";
const listeners = new Set<() => void>();

export function applyTheme(theme: Theme | string) {
  const root = document.documentElement;
  if (theme === "light" || theme === "dark") root.dataset.theme = theme;
  else delete root.dataset.theme;
  try {
    localStorage.setItem(CACHE_KEY, theme);
  } catch {
    // Only a startup speed-up; the saved setting lives in the backend.
  }
  listeners.forEach((notify) => notify());
}

/** The theme remembered from last time, applied before the backend answers. */
export function cachedTheme(): Theme {
  try {
    const value = localStorage.getItem(CACHE_KEY);
    return value === "light" || value === "dark" ? value : "system";
  } catch {
    return "system";
  }
}

export function isDark(): boolean {
  const forced = document.documentElement.dataset.theme;
  if (forced) return forced === "dark";
  return window.matchMedia(DARK_QUERY).matches;
}

/** Re-renders when the effective theme changes (setting or Windows switch). */
export function useIsDark(): boolean {
  const [dark, setDark] = useState(isDark);
  useEffect(() => {
    const update = () => setDark(isDark());
    const media = window.matchMedia(DARK_QUERY);
    media.addEventListener("change", update);
    listeners.add(update);
    return () => {
      media.removeEventListener("change", update);
      listeners.delete(update);
    };
  }, []);
  return dark;
}
