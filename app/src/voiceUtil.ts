// Small helpers shared by the voice card, the setup and the pill.

import { useEffect, useRef, useState } from "react";
import { onVoiceDownload, type DownloadProgress, type EngineView } from "./api";
import { language, resolveLanguage, type Lang } from "./i18n";

export function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  if (bytes >= 1024 * 1024) return `${Math.round(bytes / (1024 * 1024))} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}

/** The recognition language a setting resolves to ("auto" follows the app language). */
export function recognitionLanguage(setting: string): Lang {
  return setting === "auto" ? language() : resolveLanguage(setting);
}

export function engineFor(engines: EngineView[], lang: string): EngineView | undefined {
  const matches = engines.filter((e) => e.info.languages.includes(lang));
  return matches.find((e) => e.recommended) ?? matches[0];
}

/** Why a model download failed, from the backend's English message. */
export function downloadErrorKind(message: string): "cancelled" | "offline" | "other" {
  if (/cancel/i.test(message)) return "cancelled";
  if (/offline|network|internet|connect|resolve|dns|timed? ?out|unreachable|reach/i.test(message)) return "offline";
  return "other";
}

/** Why the microphone test failed. */
export function micErrorKind(message: string): "permission" | "nodevice" | "other" {
  if (/permission|denied|not allowed|privacy|access/i.test(message)) return "permission";
  if (/no (input |audio )?(device|microphone)|not found|unplugged|disconnect|no microphone/i.test(message)) return "nodevice";
  return "other";
}

const MODIFIERS = ["ctrl", "control", "alt", "option", "shift", "cmd", "command", "super", "meta", "win", "cmdorctrl", "commandorcontrol"];

/** A push-to-talk key must be modifiers plus one key (or a function key on its own). */
export function pushKeyProblem(key: string): "empty" | "invalid" | null {
  const parts = key.split("+").map((p) => p.trim());
  if (!key.trim()) return "empty";
  if (parts.some((p) => p === "")) return "invalid";
  const last = parts[parts.length - 1].toLowerCase();
  const mods = parts.slice(0, -1).map((p) => p.toLowerCase());
  if (MODIFIERS.includes(last)) return "invalid";
  if (mods.some((m) => !MODIFIERS.includes(m))) return "invalid";
  if (mods.length === 0 && !/^f([1-9]|1\d|2[0-4])$/i.test(last)) return "invalid";
  return null;
}

/** "Ctrl+Alt+Space" from a key press, or null while only modifiers are held. */
export function comboFromEvent(e: KeyboardEvent, mac: boolean): string | null {
  if (["Control", "Alt", "Shift", "Meta", "OS"].includes(e.key)) return null;
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push(mac ? "Option" : "Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push(mac ? "Cmd" : "Super");
  let key = e.code.startsWith("Key") ? e.code.slice(3) : e.code.startsWith("Digit") ? e.code.slice(5) : e.code;
  if (e.code === "Space") key = "Space";
  parts.push(key);
  return parts.join("+");
}

/** Download progress per engine id, from `localflow://voice-download`. Cleared when `forget` is called. */
export function useDownloads(onFinished?: (p: DownloadProgress) => void) {
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>({});
  const callback = useRef(onFinished);
  callback.current = onFinished;
  useEffect(() => {
    const un = onVoiceDownload((p) => {
      setProgress((m) => ({ ...m, [p.engine]: p }));
      if (p.finished) callback.current?.(p);
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);
  const forget = (engine: string) =>
    setProgress((m) => {
      const { [engine]: _gone, ...rest } = m;
      return rest;
    });
  return { progress, forget };
}
