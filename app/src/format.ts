// Date and duration formatting. The backend stores UTC; everything is shown in local time.

const timeFormat = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
const relativeFormat = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

export function formatTime(iso: string | null | undefined): string {
  return iso ? timeFormat.format(new Date(iso)) : "—";
}

/** "3 minutes ago", "in 2 hours", ... */
export function formatRelative(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return "—";
  const seconds = Math.round((new Date(iso).getTime() - now) / 1000);
  const units: [Intl.RelativeTimeFormatUnit, number][] = [
    ["day", 86400],
    ["hour", 3600],
    ["minute", 60],
  ];
  for (const [unit, size] of units) {
    if (Math.abs(seconds) >= size) return relativeFormat.format(Math.round(seconds / size), unit);
  }
  return relativeFormat.format(seconds, "second");
}

export function formatDuration(startIso: string, endIso: string | null): string {
  if (!endIso) return "—";
  return formatMs(new Date(endIso).getTime() - new Date(startIso).getTime());
}

export function formatMs(ms: number): string {
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(2)} s`;
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
export const SCHEDULE_PRESETS: { label: string; value: string }[] = [
  { label: "Manual only", value: "" },
  { label: "Every 5 minutes", value: "0 */5 * * * *" },
  { label: "Every 15 minutes", value: "0 */15 * * * *" },
  { label: "Every 30 minutes", value: "0 */30 * * * *" },
  { label: "Every hour", value: "0 0 * * * *" },
  { label: "Every day at 9:00", value: "0 0 9 * * *" },
  { label: "Every day at 18:00", value: "0 0 18 * * *" },
  { label: "Weekdays at 9:00", value: "0 0 9 * * Mon-Fri" },
  { label: "Every Monday at 9:00", value: "0 0 9 * * Mon" },
];

export function describeSchedule(schedule: string | null): string {
  if (!schedule) return "Manual only";
  return SCHEDULE_PRESETS.find((p) => p.value === schedule)?.label ?? schedule;
}
