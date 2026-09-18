// Date and duration formatting. The backend stores UTC; everything is shown in local time,
// in the language chosen in Settings.

import { locale, t, type Key } from "./i18n";
import { parseTriggers, type ExtraTriggers } from "./api";

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
  triggers?: string | ExtraTriggers | null;
}): string {
  if (!a.enabled) return t("trigger.disabled");
  const parts: string[] = [];
  const extra = typeof a.triggers === "string" || a.triggers == null ? parseTriggers(a.triggers) : a.triggers;
  if (extra.hotkey) parts.push(extra.hotkey);
  if (extra.app_start) parts.push(t("trigger.appStart", { app: extra.app_start }));
  if (extra.app_exit) parts.push(t("trigger.appExit", { app: extra.app_exit }));
  if (extra.idle_minutes) parts.push(t("trigger.idle", { n: extra.idle_minutes }));
  if (extra.usb) parts.push(t("trigger.usb"));
  if (extra.after?.automation_id) parts.push(t("trigger.after"));
  if (a.watch_path) parts.push(t("trigger.watching", { path: a.watch_path + (a.watch_pattern ? ` (${a.watch_pattern})` : "") }));
  if (a.schedule) parts.push(describeSchedule(a.schedule));
  if (a.run_on_startup) parts.push(t("trigger.onStartup"));
  return parts.length ? parts.join(" · ") : t("trigger.manual");
}

export function describeSchedule(schedule: string | null): string {
  if (!schedule) return t("trigger.manualOnly");
  const preset = PRESETS.find((p) => p.value === schedule);
  return preset ? t(preset.key) : (humanizeCron(schedule) ?? schedule);
}

const DAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"] as const;

/** "mon", "1" or "7" -> "Mon"; undefined if it isn't one day. */
function dayName(field: string): (typeof DAYS)[number] | undefined {
  const lower = field.toLowerCase();
  const byName = DAYS.find((d) => d.toLowerCase() === lower.slice(0, 3) && lower.length >= 3);
  if (byName) return byName;
  const n = Number(field);
  return Number.isInteger(n) && n >= 0 && n <= 7 ? DAYS[n % 7] : undefined;
}

/**
 * Common schedules in words: every minute, every 10 minutes, every 2 hours,
 * every day at 20:30, weekdays at 9:00, every Sunday at 12:00.
 * Anything else returns undefined and is shown as written.
 */
export function humanizeCron(schedule: string): string | undefined {
  const parts = schedule.trim().split(/\s+/);
  if (parts.length !== 6) return undefined;
  const [sec, min, hour, dom, month, dow] = parts;
  if (dom !== "*" || month !== "*") return undefined;
  const everyDay = dow === "*" || dow === "?";
  const num = (s: string) => (/^\d{1,2}$/.test(s) ? Number(s) : undefined);
  const step = (s: string) => /^\*\/(\d+)$/.exec(s)?.[1];

  if (everyDay && sec === "*" && min === "*" && hour === "*") return t("cron.everySecond");
  if (everyDay && num(sec) !== undefined && min === "*" && hour === "*") return t("cron.everyMinute");
  if (everyDay && num(sec) !== undefined && step(min) && hour === "*") return t("cron.everyNMinutes", { n: step(min)! });
  if (everyDay && num(sec) !== undefined && num(min) !== undefined && step(hour)) return t("cron.everyNHours", { n: step(hour)! });

  const h = num(hour);
  const m = num(min);
  if (num(sec) === undefined || h === undefined || m === undefined || h > 23 || m > 59) return undefined;
  const time = `${h}:${String(m).padStart(2, "0")}`;
  if (everyDay) return t("cron.dailyAt", { time });
  const lower = dow.toLowerCase();
  if (lower === "mon-fri" || lower === "1-5") return t("cron.weekdaysAt", { time });
  if (["sat,sun", "sun,sat", "0,6", "6,0", "6,7"].includes(lower)) return t("cron.weekendsAt", { time });
  const day = dayName(dow);
  return day ? t("cron.weeklyAt", { day: t(`cron.day.${day}` as Key), time }) : undefined;
}

/** Translated status word ("success", "failed", ...). */
export function statusLabel(status: string): string {
  const key = `status.${status}` as Key;
  return ["success", "failed", "running", "never"].includes(status) ? t(key) : status;
}
