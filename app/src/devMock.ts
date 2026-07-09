// Browser preview mode: when the UI runs in a normal browser (`npm run dev` without Tauri),
// fake the Rust backend with in-memory data so the interface can be developed and checked.
// Never included in the desktop app: main.tsx only loads this outside Tauri in dev builds.

import { mockIPC } from "@tauri-apps/api/mocks";
import type { AutomationRun, AutomationSummary, LogEntry } from "./api";

const now = Date.now();
const iso = (offsetMs: number) => new Date(now + offsetMs).toISOString();

const templates = [
  { slug: "hello-world", title: "Hello world", description: "The smallest possible automation.", schedule: "", code: 'log("hello")' },
  { slug: "organize-pdfs", title: "Organize PDF files", description: "Move PDFs from Downloads into Documents/PDF.", schedule: "0 0 * * * *", code: "-- organize" },
];

let runs: AutomationRun[] = [
  { id: 2, automation_id: 1, status: "success", output: "[info] Moved file: report.pdf\n[notify] Organized 1 PDF file(s)", error: null, started_at: iso(-600_000), finished_at: iso(-599_950) },
  { id: 1, automation_id: 2, status: "failed", output: "", error: "fs.list: directory not found: ~/Desktop", started_at: iso(-3_600_000), finished_at: iso(-3_599_990) },
];

let automations: AutomationSummary[] = [
  {
    id: 1, name: "Organize PDF files", description: "Move PDFs from Downloads into Documents/PDF.",
    lua_code: 'automation {\n    name = "Organize PDF files",\n\n    run = function(ctx)\n        local files = fs.list("~/Downloads", "*.pdf")\n        for _, file in ipairs(files) do\n            fs.move(file, "~/Documents/PDF/" .. fs.basename(file))\n            log("Moved file: " .. file)\n        end\n    end\n}\n',
    schedule: "0 0 * * * *", enabled: true, created_at: iso(-86_400_000), updated_at: iso(-86_400_000),
    last_run: runs[0], next_run: iso(1_500_000),
  },
  {
    id: 2, name: "Tidy screenshots", description: "", lua_code: 'log("tidy")', schedule: "0 */30 * * * *",
    enabled: true, created_at: iso(-86_400_000), updated_at: iso(-86_400_000), last_run: runs[1], next_run: iso(600_000),
  },
  {
    id: 3, name: "Back up notes", description: "", lua_code: 'log("backup")', schedule: null,
    enabled: false, created_at: iso(-86_400_000), updated_at: iso(-86_400_000), last_run: null, next_run: null,
  },
];

const logs: LogEntry[] = [
  { id: 2, automation_id: 1, level: "notify", message: "Organized 1 PDF file(s)", created_at: iso(-599_950) },
  { id: 1, automation_id: 1, level: "info", message: "Moved file: C:\\Users\\you\\Downloads\\report.pdf", created_at: iso(-599_960) },
];

type Args = Record<string, any>;

export function installDevMock() {
  mockIPC((cmd, payload) => {
    const args = (payload ?? {}) as Args;
    const find = () => automations.find((a) => a.id === args.id)!;
    switch (cmd) {
      case "list_automations": return automations;
      case "get_automation": { const a = find(); return { ...a, scheduled: !!a.next_run }; }
      case "get_templates": return templates;
      case "validate_code": return /\bif\s+then\b/.test(args.code) ? "Lua syntax error: automation:1: unexpected symbol near 'then'" : null;
      case "validate_schedule": return args.schedule && args.schedule.trim().split(/\s+/).length !== 6 ? `Invalid schedule "${args.schedule}".` : null;
      case "list_runs": return runs.filter((r) => r.automation_id === args.id);
      case "list_logs": return logs.filter((l) => l.automation_id === args.id);
      case "test_run": return { success: true, logs: [{ level: "info", message: "Hello from the preview" }], error: null, duration_ms: 12 };
      case "run_automation": {
        const run: AutomationRun = { id: runs.length + 1, automation_id: args.id, status: "success", output: "[info] Done", error: null, started_at: iso(0), finished_at: iso(30) };
        runs = [run, ...runs];
        return run;
      }
      case "set_enabled": { const a = find(); a.enabled = args.enabled; a.next_run = a.enabled && a.schedule ? iso(600_000) : null; return a; }
      case "create_automation": {
        const a: AutomationSummary = { ...args.input, id: automations.length + 1, created_at: iso(0), updated_at: iso(0), last_run: null, next_run: null };
        automations = [...automations, a];
        return a;
      }
      case "update_automation": { Object.assign(find(), args.input); return find(); }
      case "delete_automation": automations = automations.filter((a) => a.id !== args.id); return null;
      case "get_settings": return { autostart: true, notifications: true, allowed_dirs: ["C:\\Users\\you"], data_dir: "C:\\Users\\you\\AppData\\Roaming\\com.hopmalbhblu.localflow", version: "2.0.0" };
      case "plugin:event|listen": return 1;
      default: return null;
    }
  });
}
