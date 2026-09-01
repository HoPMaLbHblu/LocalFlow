// Typed wrappers around the Rust commands in src-tauri/src/commands.rs and settings.rs.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Automation {
  id: number;
  name: string;
  description: string;
  lua_code: string;
  schedule: string | null;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface AutomationRun {
  id: number;
  automation_id: number;
  status: "running" | "success" | "failed";
  output: string | null;
  error: string | null;
  started_at: string;
  finished_at: string | null;
}

export interface AutomationSummary extends Automation {
  last_run: AutomationRun | null;
  next_run: string | null;
}

export interface AutomationDetail extends Automation {
  scheduled: boolean;
  next_run: string | null;
}

export interface LogEntry {
  id: number;
  automation_id: number;
  level: string;
  message: string;
  created_at: string;
}

export interface LogLine {
  level: string;
  message: string;
}

export interface Template {
  slug: string;
  title: string;
  description: string;
  schedule: string;
  code: string;
}

export interface TestRunResult {
  success: boolean;
  logs: LogLine[];
  error: string | null;
  duration_ms: number;
}

export interface Settings {
  autostart: boolean;
  notifications: boolean;
  allowed_dirs: string[];
  data_dir: string;
  version: string;
}

export interface AutomationInput {
  name: string;
  description: string;
  lua_code: string;
  schedule: string | null;
  enabled: boolean;
}

export type CommandError =
  | { kind: "validation"; messages: string[] }
  | { kind: "not_found" }
  | { kind: "error"; message: string };

/** Readable messages for any error thrown by a command. */
export function errorMessages(e: unknown): string[] {
  const err = e as CommandError;
  if (err && typeof err === "object" && "kind" in err) {
    if (err.kind === "validation") return err.messages;
    if (err.kind === "not_found") return ["This automation no longer exists."];
    return [err.message];
  }
  return [String(e)];
}

export type CoreEvent =
  | { type: "run_started"; automation_id: number; run_id: number; name: string; trigger: string }
  | { type: "log"; automation_id: number | null; run_id: number | null; level: string; message: string }
  | { type: "run_finished"; automation_id: number; name: string; trigger: string; run: AutomationRun }
  | { type: "automations_changed" };

export function onCoreEvent(handler: (event: CoreEvent) => void): Promise<UnlistenFn> {
  return listen<CoreEvent>("localflow://event", (e) => handler(e.payload));
}

export const api = {
  listAutomations: () => invoke<AutomationSummary[]>("list_automations"),
  getAutomation: (id: number) => invoke<AutomationDetail>("get_automation", { id }),
  createAutomation: (input: AutomationInput) => invoke<Automation>("create_automation", { input }),
  updateAutomation: (id: number, input: AutomationInput) =>
    invoke<Automation>("update_automation", { id, input }),
  setEnabled: (id: number, enabled: boolean) => invoke<Automation>("set_enabled", { id, enabled }),
  deleteAutomation: (id: number) => invoke<void>("delete_automation", { id }),
  runAutomation: (id: number) => invoke<AutomationRun>("run_automation", { id }),
  testRun: (code: string, name: string) => invoke<TestRunResult>("test_run", { code, name }),
  validateCode: (code: string) => invoke<string | null>("validate_code", { code }),
  validateSchedule: (schedule: string) => invoke<string | null>("validate_schedule", { schedule }),
  listRuns: (id: number, limit = 100) => invoke<AutomationRun[]>("list_runs", { id, limit }),
  listLogs: (id: number, limit = 500) => invoke<LogEntry[]>("list_logs", { id, limit }),
  getTemplates: () => invoke<Template[]>("get_templates"),
  getSettings: () => invoke<Settings>("get_settings"),
  setAutostart: (enabled: boolean) => invoke<void>("set_autostart", { enabled }),
  setNotifications: (enabled: boolean) => invoke<void>("set_notifications", { enabled }),
  setAllowedDirs: (dirs: string[]) => invoke<void>("set_allowed_dirs", { dirs }),
};
