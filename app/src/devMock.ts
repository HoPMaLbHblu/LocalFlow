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
    schedule: "0 0 * * * *", enabled: true, created_at: iso(-86_400_000), updated_at: iso(-86_400_000), run_on_startup: false, watch_path: null, watch_pattern: null,
    last_run: runs[0], next_run: iso(1_500_000),
  },
  {
    id: 2, name: "Tidy screenshots", description: "", lua_code: 'log("tidy")', schedule: "0 */30 * * * *",
    enabled: true, created_at: iso(-86_400_000), updated_at: iso(-86_400_000), run_on_startup: false, watch_path: null, watch_pattern: null, last_run: runs[1], next_run: iso(600_000),
  },
  {
    id: 3, name: "Back up notes", description: "", lua_code: 'log("backup")', schedule: null,
    enabled: false, created_at: iso(-86_400_000), updated_at: iso(-86_400_000), run_on_startup: true, watch_path: null, watch_pattern: null, last_run: null, next_run: null,
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
      case "get_automation": { const a = find(); return { ...a, scheduled: !!a.next_run, watching: !!a.watch_path }; }
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
      case "get_settings": return { autostart: true, notifications: true, allowed_dirs: ["C:\\Users\\you"], language: "auto", theme: "system", script_timeout_secs: 30, data_dir: "C:\\Users\\you\\AppData\\Roaming\\com.hopmalbhblu.localflow", version: "1.0.0" };
      case "plugin:event|listen": return 1;
      case "set_language": case "set_theme": case "set_script_timeout": case "export_automation": return null;
      case "take_pending_import": return null;
      case "get_metrics": {
        const minutes = (args as { minutes: number }).minutes;
        const now = Math.floor(Date.now() / 1000);
        return Array.from({ length: 120 }, (_, i) => {
          const at = now - minutes * 60 + ((i + 1) * minutes * 60) / 120;
          return {
            at: Math.round(at),
            cpu: Math.round(25 + 20 * Math.sin(i / 7) + (i % 5) * 3),
            memory: Math.round(55 + 8 * Math.sin(i / 19)),
            disk: 63,
            battery: Math.max(20, 100 - Math.floor(i / 2)),
          };
        });
      }
      case "list_backups": return [
        { file_name: "localflow-2026-09-29_09-00-00-daily.db", created_at: iso(-3_600_000), size: 61440, kind: "daily" },
        { file_name: "localflow-2026-09-28_18-30-00-before-update.db", created_at: iso(-86_400_000), size: 53248, kind: "before-update" },
      ];
      case "list_trash": return [{
        ...automations[0], id: 99, name: "Old experiment", enabled: false,
        deleted_at: iso(-2 * 86_400_000), last_run: null, next_run: null,
      }];
      case "restore_automation": return { ...automations[0], id: 99, name: "Old experiment" };
      case "delete_forever": return null;
      case "list_versions": return [{
        id: 1, automation_id: args.id, name: find()?.name ?? "Earlier", description: "", lua_code: "log('older version')",
        schedule: null, run_on_startup: false, watch_path: null, watch_pattern: null, triggers: null, saved_at: iso(-3_600_000),
      }];
      case "startup_notice": return null;
      case "plugin:dialog|open": return "C:\\Users\\you\\Downloads\\Cleaner.localflow";
      case "plugin:dialog|save": return "C:\\Users\\you\\Documents\\export.localflow";
      case "preview_import": return {
        automation: { format: "localflow", version: 1, name: "Downloads cleaner", description: "Deletes old installers and opens Explorer.",
          lua_code: 'for _, f in ipairs(fs.find("~/Downloads", "*.exe")) do\n    fs.delete(f)\nend\napp.open("~/Downloads")\n',
          schedule: "0 0 9 * * Mon", run_on_startup: false, watch_path: null, watch_pattern: null, app_version: "1.0.0" },
        risks: ["deletes_files", "opens_apps", "runs_on_schedule"], problems: [] };
      case "import_automation": {
        const a: AutomationSummary = { id: automations.length + 1, name: "Downloads cleaner", description: "", lua_code: "log(1)", schedule: "0 0 9 * * Mon",
          enabled: false, created_at: iso(0), updated_at: iso(0), run_on_startup: false, watch_path: null, watch_pattern: null, last_run: null, next_run: null };
        automations = [...automations, a];
        return a;
      }
      default: return null;
    }
  });
}
