// Typed wrappers around the Rust commands in src-tauri/src/commands.rs and settings.rs.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { t, translateBackendMessage } from "./i18n";

export interface Automation {
  id: number;
  name: string;
  description: string;
  lua_code: string;
  schedule: string | null;
  enabled: boolean;
  created_at: string;
  updated_at: string;
  run_on_startup: boolean;
  watch_path: string | null;
  watch_pattern: string | null;
  /** Set when the automation is in the trash. */
  deleted_at?: string | null;
}

/** An earlier saved state of an automation. */
export interface AutomationVersion {
  id: number;
  automation_id: number;
  name: string;
  description: string;
  lua_code: string;
  schedule: string | null;
  run_on_startup: boolean;
  watch_path: string | null;
  watch_pattern: string | null;
  /** When this version was replaced by a newer one. */
  saved_at: string;
}

export interface BackupInfo {
  file_name: string;
  created_at: string;
  size: number;
  kind: "daily" | "before-update" | "manual" | "before-restore" | "before-delete" | "damaged" | string;
}

export type StartupNotice = { kind: "restored"; backup: string } | { kind: "recovered_from_damage"; backup: string };

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
  watching: boolean;
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
  run_on_startup?: boolean;
  watch_path?: string;
  watch_pattern?: string;
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
  /** "auto", "en", "ru" or "de". */
  language: string;
  /** "system", "light" or "dark". */
  theme: string;
  /** How long a script may run, in seconds. */
  script_timeout_secs: number;
  data_dir: string;
  version: string;
}

export interface AutomationInput {
  name: string;
  description: string;
  lua_code: string;
  schedule: string | null;
  enabled: boolean;
  run_on_startup: boolean;
  watch_path: string | null;
  watch_pattern: string | null;
}

/** Contents of a .localflow file. */
export interface SharedAutomation {
  format: string;
  version: number;
  name: string;
  description: string;
  lua_code: string;
  schedule: string | null;
  run_on_startup: boolean;
  watch_path: string | null;
  watch_pattern: string | null;
  exported_at?: string;
  app_version?: string;
}

export type Risk =
  | "deletes_files"
  | "moves_files"
  | "writes_files"
  | "opens_apps"
  | "uses_internet"
  | "uses_clipboard"
  | "runs_on_startup"
  | "watches_folder"
  | "runs_on_schedule";

export interface ImportPreview {
  automation: SharedAutomation;
  risks: Risk[];
  /** Reasons the file can't be imported as it is (e.g. a Lua syntax error). */
  problems: string[];
}

export type CommandError =
  | { kind: "validation"; messages: string[] }
  | { kind: "not_found" }
  | { kind: "error"; message: string };

/** Readable messages for any error thrown by a command. */
export function errorMessages(e: unknown): string[] {
  const err = e as CommandError;
  if (err && typeof err === "object" && "kind" in err) {
    if (err.kind === "validation") return err.messages.map(translateBackendMessage);
    if (err.kind === "not_found") return [t("backend.notFound")];
    return [translateBackendMessage(err.message)];
  }
  return [String(e)];
}

export type CoreEvent =
  | { type: "run_started"; automation_id: number; run_id: number; name: string; trigger: string }
  | { type: "log"; automation_id: number | null; run_id: number | null; level: string; message: string }
  | { type: "run_finished"; automation_id: number; name: string; trigger: string; run: AutomationRun }
  | { type: "automations_changed" };

/** A .localflow file was opened while LocalFlow was already running. */
export function onOpenFile(handler: (path: string) => void): Promise<UnlistenFn> {
  return listen<string>("localflow://open-file", (e) => handler(e.payload));
}

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
  setLanguage: (language: string) => invoke<void>("set_language", { language }),
  setTheme: (theme: string) => invoke<void>("set_theme", { theme }),
  setScriptTimeout: (seconds: number) => invoke<void>("set_script_timeout", { seconds }),
  exportAutomation: (id: number, path: string) => invoke<void>("export_automation", { id, path }),
  previewImport: (path: string) => invoke<ImportPreview>("preview_import", { path }),
  importAutomation: (path: string) => invoke<Automation>("import_automation", { path }),
  takePendingImport: () => invoke<string | null>("take_pending_import"),
  listTrash: () => invoke<Automation[]>("list_trash"),
  restoreAutomation: (id: number) => invoke<Automation>("restore_automation", { id }),
  deleteForever: (id: number) => invoke<void>("delete_forever", { id }),
  listVersions: (id: number) => invoke<AutomationVersion[]>("list_versions", { id }),
  restoreVersion: (id: number, versionId: number) => invoke<Automation>("restore_version", { id, versionId }),
  listBackups: () => invoke<BackupInfo[]>("list_backups"),
  backupNow: () => invoke<BackupInfo>("backup_now"),
  restoreBackup: (fileName: string) => invoke<void>("restore_backup", { fileName }),
  openBackupsFolder: () => invoke<void>("open_backups_folder"),
  startupNotice: () => invoke<StartupNotice | null>("startup_notice"),
};
