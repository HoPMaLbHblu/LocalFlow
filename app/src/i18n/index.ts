// Tiny translation layer. The whole app re-renders when the language changes
// (App is keyed on it), so components can call t() directly.

import { en, type Key, type Dictionary } from "./en";
import { ru } from "./ru";
import { de } from "./de";

export type Lang = "en" | "ru" | "de";
export type LanguageSetting = "auto" | Lang;

export const LANGUAGES: { code: Lang; name: string }[] = [
  { code: "en", name: "English" },
  { code: "ru", name: "Русский" },
  { code: "de", name: "Deutsch" },
];

const DICTIONARIES: Record<Lang, Dictionary> = { en, ru, de };
const CACHE_KEY = "localflow.language";

let current: Lang = "en";

/** "auto" becomes the Windows/browser language if we have it, otherwise English. */
export function resolveLanguage(setting: LanguageSetting | string | null | undefined): Lang {
  const wanted = !setting || setting === "auto" ? navigator.language : setting;
  const prefix = wanted.split("-")[0].toLowerCase();
  return (LANGUAGES.find((l) => l.code === prefix)?.code ?? "en") as Lang;
}

export function setLanguage(setting: LanguageSetting | string): Lang {
  current = resolveLanguage(setting);
  document.documentElement.lang = current;
  try {
    localStorage.setItem(CACHE_KEY, setting);
  } catch {
    // Only a startup speed-up; the saved setting lives in the backend.
  }
  return current;
}

/** The language setting remembered from last time, used before the backend answers. */
export function cachedLanguageSetting(): string {
  try {
    return localStorage.getItem(CACHE_KEY) ?? "auto";
  } catch {
    return "auto";
  }
}

export function language(): Lang {
  return current;
}

/** Locale for dates and relative times. */
export function locale(): string {
  return current;
}

export function t(key: Key, vars?: Record<string, string | number>): string {
  let text = DICTIONARIES[current][key] ?? en[key] ?? key;
  if (vars) {
    for (const [name, value] of Object.entries(vars)) text = text.split(`{${name}}`).join(String(value));
  }
  return text;
}

/** Look up a key that is built at runtime (e.g. template slugs); undefined if unknown. */
export function tMaybe(key: string, vars?: Record<string, string | number>): string | undefined {
  return key in en ? t(key as Key, vars) : undefined;
}

export type { Key };

// Messages the Rust side sends in English, and how to translate them.
const BACKEND_MESSAGES: [RegExp, (m: RegExpMatchArray) => string][] = [
  [/^Name is required\.?$/, () => t("backend.nameRequired")],
  [/^Name must be at most 100 characters\.?$/, () => t("backend.nameTooLong")],
  [/^Lua code must not be empty\.?$/, () => t("backend.codeEmpty")],
  [/^Lua syntax error: ([\s\S]*)$/, (m) => t("backend.syntax", { detail: m[1] })],
  [/^Invalid schedule "(.*)"\./, (m) => t("backend.schedule", { value: m[1] })],
  [/^Watch folder: access denied: '(.*)' is outside/, (m) => t("backend.watchDenied", { path: m[1] })],
  [/^Watch folder not found: (.*)$/, (m) => t("backend.watchMissing", { path: m[1] })],
  [/^Folder not found: (.*)$/, (m) => t("backend.folderMissing", { path: m[1] })],
  [/^Add at least one folder\.?$/, () => t("backend.folderRequired")],
];

export function translateBackendMessage(message: string): string {
  for (const [pattern, translate] of BACKEND_MESSAGES) {
    const match = message.match(pattern);
    if (match) return translate(match);
  }
  return message;
}
