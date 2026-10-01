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
  /** "Allow system control": commands, keystrokes, shutdown, ... */
  allow_system?: boolean;
  /** JSON of ExtraTriggers, or null. */
  triggers?: string | null;
}

/** Hotkey, app start/exit, idle and USB triggers. */
export interface ExtraTriggers {
  hotkey?: string | null;
  app_start?: string | null;
  app_exit?: string | null;
  idle_minutes?: number | null;
  usb?: boolean;
  /** Run when another automation finishes. */
  after?: { automation_id: number; when: "success" | "failure" | "always" } | null;
}

export function parseTriggers(json: string | null | undefined): ExtraTriggers {
  if (!json) return {};
  try {
    return JSON.parse(json) as ExtraTriggers;
  } catch {
    return {};
  }
}

/** Empty strings and zero become "not set", so forms compare correctly. */
export function cleanTriggers(tr: ExtraTriggers): ExtraTriggers {
  const out: ExtraTriggers = {};
  if (tr.hotkey?.trim()) out.hotkey = tr.hotkey.trim();
  if (tr.app_start?.trim()) out.app_start = tr.app_start.trim();
  if (tr.app_exit?.trim()) out.app_exit = tr.app_exit.trim();
  if (tr.idle_minutes && tr.idle_minutes > 0) out.idle_minutes = tr.idle_minutes;
  if (tr.usb) out.usb = true;
  if (tr.after?.automation_id) out.after = { automation_id: tr.after.automation_id, when: tr.after.when || "success" };
  return out;
}

/** Settings › AI. The key itself never reaches the window. */
export interface AiSettings {
  configured: boolean;
  scope: string;
  model: string;
  scopes: string[];
  models: string[];
  cache_entries: number;
}

/** A link set on the Links page. */
export interface Link {
  url: string;
  title: string;
}

export interface LinkSet {
  name: string;
  links: Link[];
  browser: string;
  new_window: boolean;
  updated_at: number;
}

/** Settings › Telegram and Discord. Secrets never reach the window. */
export interface BotSettings {
  telegram_token: boolean;
  telegram_chat: string | null;
  discord_webhook: boolean;
  remote: boolean;
  remote_power: boolean;
}

export interface FoundChat {
  id: string;
  name: string;
}

/** One minute of system history; values are percentages. */
export interface MetricSample {
  at: number;
  cpu: number;
  memory: number;
  disk: number;
  battery: number | null;
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
  /** Where it's listed in the picker (files, photos, system, ...). */
  category?: string;
  title: string;
  description: string;
  schedule: string;
  code: string;
  run_on_startup?: boolean;
  watch_path?: string;
  watch_pattern?: string;
  allow_system?: boolean;
  /** JSON of ExtraTriggers ("" for none). */
  triggers?: string;
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
  allow_system: boolean;
  triggers: ExtraTriggers;
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
  | "runs_on_schedule"
  | "runs_commands"
  | "controls_input"
  | "controls_power"
  | "needs_system_control"
  | "runs_on_events";

export interface ImportPreview {
  automation: SharedAutomation;
  risks: Risk[];
  /** Reasons the file can't be imported as it is (e.g. a Lua syntax error). */
  problems: string[];
}

// ---- Dota 2 companion (src-tauri/src/dota.rs) ----

export type DotaTeam = "radiant" | "dire";
export type DotaRole = "carry" | "mid" | "offlane" | "soft_support" | "hard_support";
export type DotaPhase = "unknown" | "menu" | "loading" | "hero_selection" | "strategy_time" | "pre_game" | "playing" | "post_game";

/** Where a piece of advice comes from: statistics, or a rule of thumb. */
export type DotaEvidence =
  | { kind: "sourced"; source: string; detail: string; fetched_at: number }
  | { kind: "heuristic"; rule: string };

export interface DotaReason {
  text: string;
  evidence: DotaEvidence;
}

export interface DotaHeroSuggestion {
  hero_id: number;
  hero: string;
  /** For ordering only; not a win probability. */
  score: number;
  reasons: DotaReason[];
}

export interface DotaItemAdvice {
  item: string;
  key: string;
  priority: number;
  why: string;
  evidence: DotaEvidence;
  alternatives: string[];
}

export interface DotaItemPlan {
  hero_id: number;
  hero: string;
  starting: DotaItemAdvice[];
  core: DotaItemAdvice[];
  situational: DotaItemAdvice[];
  adaptations: DotaReason[];
  data_note: string;
}

export interface DotaSlot {
  /** 1-5 */
  slot: number;
  hero_id: number | null;
  hero: string | null;
  confidence: number;
  source: "screenshot" | "manual" | "gsi" | null;
  uncertain: boolean;
  alternatives: { hero_id: number; hero: string; confidence: number }[];
}

export interface DotaDraft {
  allies: DotaSlot[];
  enemies: DotaSlot[];
  player_hero_id: number | null;
  player_hero: string | null;
  team: DotaTeam;
  team_assumed: boolean;
  role: DotaRole | null;
  captures: number;
  updated_at: number;
  complete: boolean;
  uncertain: string[];
  note: string | null;
}

export interface DotaCapture {
  draft: DotaDraft;
  recognized: number;
  layout: string;
  width: number;
  height: number;
  warnings: string[];
  image: string;
}

export interface DotaSourceInfo {
  name: string;
  fetched_at: number | null;
  patch: string | null;
  offline: boolean;
  note: string;
}

export interface DotaStatus {
  gsi_installed: boolean;
  dota_found: boolean;
  dota_dir: string | null;
  cfg_path: string | null;
  listening: boolean;
  port: number;
  listen_error: string | null;
  dota_running: boolean;
  in_menu: boolean;
  state: DotaPhase;
  team: DotaTeam | null;
  hero_id: number | null;
  hero_name: string | null;
  last_update: number | null;
  launch_assistant: boolean;
  launch_url: string;
  source: DotaSourceInfo;
}

export interface DotaHero {
  id: number;
  name: string;
  localized_name: string;
  primary_attr: string;
  attack_type: string;
  roles: string[];
}

export interface DotaSettings {
  launch_url: string;
  role: DotaRole | null;
  gsi_port: number;
  launch_assistant: boolean;
  /** The player's Dota account id (Steam32) for the post-game review. */
  account_id: number | null;
  /** Live match helper (needs Game State Integration). */
  live_helper: boolean;
  /** The Game State Integration file's text, to copy by hand if needed. */
  cfg_text: string;
}

/** What Game State Integration says about the player during a match. */
export interface DotaLiveState {
  /** Game clock in seconds (negative before the horn). */
  clock: number;
  gold: number;
  items: string[];
  hero_id: number | null;
  alive: boolean;
  updated_at: number;
}

export interface DotaReminder {
  clock: number;
  text: string;
  kind: string;
}

export interface DotaNextItem {
  advice: DotaItemAdvice;
  missing_gold: number;
  affordable: boolean;
}

export interface DotaLive {
  state: DotaLiveState;
  hero: string | null;
  next_item: DotaNextItem | null;
  next_note: string | null;
  reminders: DotaReminder[];
}

export interface DotaMatchup {
  hero_id: number;
  hero: string;
  reason: DotaReason;
}

export interface DotaHeroLookup {
  hero_id: number;
  hero: string;
  strong_against: DotaMatchup[];
  weak_against: DotaMatchup[];
  common_items: DotaItemAdvice[];
  traits: string[];
  data_note: string;
}

export interface DotaMatchSummary {
  match_id: number;
  hero_id: number;
  hero: string;
  won: boolean;
  kills: number;
  deaths: number;
  assists: number;
  gpm: number;
  xpm: number;
  last_hits: number;
  duration_secs: number;
  start_time: number;
  items: string[];
}

export interface DotaBenchmark {
  metric: string;
  value: number;
  /** 0-1 compared with other players of this hero. */
  percentile: number | null;
}

export interface DotaMatchReview {
  summary: DotaMatchSummary;
  benchmarks: DotaBenchmark[];
  notes: DotaReason[];
  data_note: string;
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
  | { type: "automations_changed" }
  | { type: "notice"; message: string }
  | { type: "show_dota" }
  | { type: "dota_changed" };

/** A .localflow file was opened while LocalFlow was already running. */
export function onOpenFile(handler: (path: string) => void): Promise<UnlistenFn> {
  return listen<string>("localflow://open-file", (e) => handler(e.payload));
}

/** A newer LocalFlow release on GitHub. */
export interface UpdateInfo {
  current: string;
  latest: { version: string; url: string; name: string; published_at: string };
}

export function onUpdateAvailable(handler: (info: UpdateInfo) => void): Promise<UnlistenFn> {
  return listen<UpdateInfo>("localflow://update", (e) => handler(e.payload));
}

export function onCoreEvent(handler: (event: CoreEvent) => void): Promise<UnlistenFn> {
  return listen<CoreEvent>("localflow://event", (e) => handler(e.payload));
}

export const api = {
  updateStatus: (refresh: boolean) => invoke<UpdateInfo | null>("update_status", { refresh }),
  dismissUpdate: (version: string) => invoke<void>("dismiss_update", { version }),
  getUpdateCheck: () => invoke<boolean>("get_update_check"),
  setUpdateCheck: (enabled: boolean) => invoke<void>("set_update_check", { enabled }),
  openReleasePage: (url: string) => invoke<void>("open_release_page", { url }),
  listAutomations: () => invoke<AutomationSummary[]>("list_automations"),
  getAutomation: (id: number) => invoke<AutomationDetail>("get_automation", { id }),
  createAutomation: (input: AutomationInput) => invoke<Automation>("create_automation", { input }),
  updateAutomation: (id: number, input: AutomationInput) =>
    invoke<Automation>("update_automation", { id, input }),
  setEnabled: (id: number, enabled: boolean) => invoke<Automation>("set_enabled", { id, enabled }),
  deleteAutomation: (id: number) => invoke<void>("delete_automation", { id }),
  runAutomation: (id: number) => invoke<AutomationRun>("run_automation", { id }),
  testRun: (code: string, name: string, allowSystem = false) =>
    invoke<TestRunResult>("test_run", { code, name, allowSystem }),
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
  getMetrics: (minutes: number, points: number) => invoke<MetricSample[]>("get_metrics", { minutes, points }),
  getAiSettings: () => invoke<AiSettings>("get_ai_settings"),
  setAiSettings: (key: string | null, scope: string, model: string) => invoke<void>("set_ai_settings", { key, scope, model }),
  clearAiKey: () => invoke<void>("clear_ai_key"),
  testAi: (language: string) => invoke<string>("test_ai", { language }),
  aiWriteAutomation: (
    description: string,
    language: string,
    currentCode: string | null,
    history: { request: string; reply: string }[] = [],
  ) =>
    invoke<{ code: string; warnings: string[]; needs_system_control: boolean; answer: string | null }>("ai_write_automation", {
      description,
      language,
      currentCode,
      history,
    }),
  clearAiCache: () => invoke<number>("clear_ai_cache"),
  getBotSettings: () => invoke<BotSettings>("get_bot_settings"),
  setBotSettings: (token: string | null, chat: string | null, webhook: string | null, remote: boolean, remotePower: boolean) =>
    invoke<void>("set_bot_settings", { token, chat, webhook, remote, remotePower }),
  clearBot: (which: "telegram" | "discord") => invoke<void>("clear_bot", { which }),
  findTelegramChats: (token: string | null) => invoke<[string, FoundChat[]]>("find_telegram_chats", { token }),
  testBots: () => invoke<string>("test_bots"),
  linksList: () => invoke<LinkSet[]>("links_list"),
  linksTrash: () => invoke<LinkSet[]>("links_trash"),
  linksSave: (set: LinkSet, oldName: string | null) => invoke<LinkSet>("links_save", { set, oldName }),
  linksDelete: (name: string) => invoke<boolean>("links_delete", { name }),
  linksRestore: (name: string) => invoke<void>("links_restore", { name }),
  linksOpen: (name: string) => invoke<number>("links_open", { name }),
  linksParse: (text: string) => invoke<Link[]>("links_parse", { text }),
  linksImportBookmarks: (folder: string, browser: string) => invoke<Link[]>("links_import_bookmarks", { folder, browser }),
  dotaKeySaved: () => invoke<boolean>("dota_key_saved"),
  dotaSetKey: (key: string | null) => invoke<void>("dota_set_key", { key }),
  backupNow: () => invoke<BackupInfo>("backup_now"),
  restoreBackup: (fileName: string) => invoke<void>("restore_backup", { fileName }),
  openBackupsFolder: () => invoke<void>("open_backups_folder"),
  startupNotice: () => invoke<StartupNotice | null>("startup_notice"),
  dotaGetSettings: () => invoke<DotaSettings>("dota_get_settings"),
  dotaSetSettings: (launchUrl: string, role: DotaRole | null, gsiPort: number, launchAssistant: boolean) =>
    invoke<void>("dota_set_settings", { launchUrl, role, gsiPort, launchAssistant }),
  dotaStatus: () => invoke<DotaStatus>("dota_status"),
  dotaInstallGsi: () => invoke<string>("dota_install_gsi"),
  dotaUninstallGsi: () => invoke<boolean>("dota_uninstall_gsi"),
  dotaDraft: () => invoke<DotaDraft>("dota_draft"),
  dotaCapture: () => invoke<DotaCapture>("dota_capture"),
  dotaCorrect: (side: "allies" | "enemies", slot: number, hero: string | null) =>
    invoke<DotaDraft>("dota_correct", { side, slot, hero }),
  dotaSetHero: (hero: string | null) => invoke<DotaDraft>("dota_set_hero", { hero }),
  dotaSetTeam: (team: DotaTeam) => invoke<DotaDraft>("dota_set_team", { team }),
  dotaSetRole: (role: DotaRole | null) => invoke<void>("dota_set_role", { role }),
  dotaReset: () => invoke<DotaDraft>("dota_reset"),
  dotaSuggest: (count = 8) => invoke<DotaHeroSuggestion[]>("dota_suggest", { count }),
  dotaBuild: (hero: string | null = null) => invoke<DotaItemPlan>("dota_build", { hero }),
  dotaHeroes: () => invoke<DotaHero[]>("dota_heroes"),
  dotaLive: () => invoke<DotaLive | null>("dota_live"),
  dotaLookup: (hero: string, count = 8) => invoke<DotaHeroLookup>("dota_lookup", { hero, count }),
  dotaLastMatch: () => invoke<DotaMatchReview>("dota_last_match"),
  dotaRecentMatches: (count = 10) => invoke<DotaMatchSummary[]>("dota_recent_matches", { count }),
  dotaParseAccount: (text: string) => invoke<number>("dota_parse_account", { text }),
  dotaSetAccount: (text: string | null) => invoke<number | null>("dota_set_account", { text }),
  dotaSetLiveHelper: (enabled: boolean) => invoke<void>("dota_set_live_helper", { enabled }),
};
