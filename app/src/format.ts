// Date and duration formatting. The backend stores UTC; everything is shown in local time,
// in the language chosen in Settings.

import { locale, t, type Key } from "./i18n";

export function formatTime(iso: string | null | undefined): string {
  if (!iso) return "—";
  return new Intl.DateTimeFormat(locale(), { dateStyle: "medium", timeStyle: "short" }).format(new Date(iso));
}

/** "3 minutes ago", "in 2 hours", ... */
export function formatRelative(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return "—";
  const relative = new Intl.RelativeTimeFormat(locale(), { numeric: "auto" });
  const seconds = Math.round((new Date(iso).getTime() - now) / 1000);
  const units: [Intl.RelativeTimeFormatUnit, number][] = [
    ["day", 86400],
    ["hour", 3600],
    ["minute", 60],
  ];
  for (const [unit, size] of units) {
    if (Math.abs(seconds) >= size) return relative.format(Math.round(seconds / size), unit);
  }
  return relative.format(seconds, "second");
}

export function formatDuration(startIso: string, endIso: string | null): string {
  if (!endIso) return "—";
  return formatMs(new Date(endIso).getTime() - new Date(startIso).getTime());
}

export function formatMs(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  return `${new Intl.NumberFormat(locale(), { maximumFractionDigits: 2 }).format(ms / 1000)} s`;
}

/** Split a stored run output ("[level] message" per line) back into lines. */
export function parseOutput(output: string | null): { level: string; message: string }[] {
  if (!output) return [];
  return output.split("\n").map((line) => {
    const match = /^\[(\w+)\] ([\s\S]*)$/.exec(line);
    return match ? { level: match[1], message: match[2] } : { level: "info", message: line };
  });
}

/** Common schedules offered in the editor. */
const PRESETS: { key: Key; value: string }[] = [
  { key: "preset.manual", value: "" },
  { key: "preset.every5", value: "0 */5 * * * *" },
  { key: "preset.every15", value: "0 */15 * * * *" },
  { key: "preset.every30", value: "0 */30 * * * *" },
  { key: "preset.hourly", value: "0 0 * * * *" },
  { key: "preset.daily9", value: "0 0 9 * * *" },
  { key: "preset.daily18", value: "0 0 18 * * *" },
  { key: "preset.weekdays9", value: "0 0 9 * * Mon-Fri" },
  { key: "preset.monday9", value: "0 0 9 * * Mon" },
];

export function schedulePresets(): { label: string; value: string }[] {
  return PRESETS.map((p) => ({ label: t(p.key), value: p.value }));
}

export function isPresetSchedule(schedule: string): boolean {
  return PRESETS.some((p) => p.value === schedule);
}

/** Short description of everything that starts an automation, e.g. "Every hour · On startup". */
export function describeTriggers(a: {
  enabled: boolean;
  schedule: string | null;
  run_on_startup: boolean;
  watch_path: string | null;
  watch_pattern?: string | null;
}): string {
  if (!a.enabled) return t("trigger.disabled");
  const parts: string[] = [];
  if (a.watch_path) parts.push(t("trigger.watching", { path: a.watch_path + (a.watch_pattern ? ` (${a.watch_pattern})` : "") }));
  if (a.schedule) parts.push(describeSchedule(a.schedule));
  if (a.run_on_startup) parts.push(t("trigger.onStartup"));
  return parts.length ? parts.join(" · ") : t("trigger.manual");
}

export function describeSchedule(schedule: string | null): string {
  if (!schedule) return t("trigger.manualOnly");
  const preset = PRESETS.find((p) => p.value === schedule);
  return preset ? t(preset.key) : schedule;
}

/** Translated status word ("success", "failed", ...). */
export function statusLabel(status: string): string {
  const key = `status.${status}` as Key;
  return ["success", "failed", "running", "never"].includes(status) ? t(key) : status;
}
