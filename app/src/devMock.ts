// Browser preview mode: when the UI runs in a normal browser (`npm run dev` without Tauri),
// fake the Rust backend with in-memory data so the interface can be developed and checked.
// Never included in the desktop app: main.tsx only loads this outside Tauri in dev builds.

import { mockIPC } from "@tauri-apps/api/mocks";
import { voiceMock } from "./devMockVoice";
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

// Dota 2 companion preview data (open http://localhost:1420/#dota for the window).
const dotaHeroes = [
  [1, "antimage", "Anti-Mage"], [2, "axe", "Axe"], [5, "crystal_maiden", "Crystal Maiden"], [8, "juggernaut", "Juggernaut"],
  [11, "nevermore", "Shadow Fiend"], [14, "pudge", "Pudge"], [26, "lion", "Lion"], [44, "phantom_assassin", "Phantom Assassin"],
  [53, "furion", "Nature's Prophet"], [74, "invoker", "Invoker"], [86, "rubick", "Rubick"], [129, "mars", "Mars"],
].map(([id, short, name]) => ({ id, name: `npc_dota_hero_${short}`, localized_name: name, primary_attr: "str", attack_type: "Melee", roles: ["Carry"] }));
const dotaSlot = (slot: number, hero_id: number | null, confidence = 0.92, source: string | null = "screenshot") => ({
  slot, hero_id, hero: dotaHeroes.find((h) => h.id === hero_id)?.localized_name ?? null, confidence: hero_id ? confidence : 0,
  source: hero_id ? source : null, uncertain: !!hero_id && confidence < 0.6 && source === "screenshot",
  alternatives: hero_id && confidence < 0.6 ? [{ hero_id: 26, hero: "Lion", confidence: 0.41 }, { hero_id: 5, hero: "Crystal Maiden", confidence: 0.22 }] : [],
});
const dotaNow = Math.floor(Date.now() / 1000);
let dotaDraft = {
  allies: [dotaSlot(1, 2, 1, "gsi"), dotaSlot(2, 86), dotaSlot(3, null), dotaSlot(4, null), dotaSlot(5, null)],
  enemies: [dotaSlot(1, 14), dotaSlot(2, 11, 0.48), dotaSlot(3, 44), dotaSlot(4, null), dotaSlot(5, null)],
  player_hero_id: 2, player_hero: "Axe", team: "radiant", team_assumed: true, role: "offlane", captures: 2,
  updated_at: dotaNow, complete: false, uncertain: ["enemies 2"], note: null,
};
let dotaAccount: number | null = null;
let dotaLiveHelper = false;
const dotaSourced = (detail: string) => ({ kind: "sourced", source: "OpenDota", detail, fetched_at: dotaNow - 7200 });

type MockLinkSet = { name: string; links: { url: string; title: string }[]; browser: string; new_window: boolean; updated_at: number };
let linkSets: MockLinkSet[] = [
  { name: "Work", browser: "chrome", new_window: true, updated_at: 0, links: Array.from({ length: 42 }, (_, i) => ({ url: `https://example.com/tab-${i + 1}`, title: i === 0 ? "Mail" : "" })) },
  { name: "Morning", browser: "default", new_window: true, updated_at: 0, links: [{ url: "https://news.ycombinator.com", title: "HN" }, { url: "https://www.bbc.com/news", title: "" }] },
];
let linkTrash: MockLinkSet[] = [];

export function installDevMock() {
  mockIPC((cmd, payload) => {
    const args = (payload ?? {}) as Args;
    const voice = voiceMock(cmd, args);
    if (voice.handled) return voice.value;
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
      // Preview only: add ?update to the address to see the "new version" banner.
      case "update_status":
        return new URLSearchParams(location.search).has("update")
          ? { current: "1.3.0", latest: { version: "1.4.0", url: "https://github.com/HoPMaLbHblu/LocalFlow/releases/latest", name: "LocalFlow v1.4.0", published_at: "" } }
          : null;
      case "get_update_check": return true;
      case "get_settings": return { autostart: true, notifications: true, allowed_dirs: ["C:\\Users\\you"], language: "auto", theme: "system", script_timeout_secs: 30, data_dir: "C:\\Users\\you\\AppData\\Roaming\\com.hopmalbhblu.localflow", version: "1.4.0" };
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
      case "get_ai_settings": return { configured: false, scope: "GIGACHAT_API_PERS", model: "GigaChat-2",
        scopes: ["GIGACHAT_API_PERS", "GIGACHAT_API_B2B", "GIGACHAT_API_CORP"], models: ["GigaChat-2", "GigaChat-2-Pro", "GigaChat-2-Max"], cache_entries: 12 };
      case "clear_ai_cache": return 12;
      case "set_ai_settings": case "clear_ai_key": return null;
      case "test_ai": return "Привет! Рад помочь.";
      case "ai_write_automation": return (args as { description: string }).description.trim().endsWith("?")
        ? { warnings: [], needs_system_control: false, answer: "fs.move never overwrites, so nothing is lost.", code: (args as { currentCode?: string }).currentCode ?? "" }
        : { warnings: [], needs_system_control: false, answer: null, code: `automation {
    name = "From AI",

    run = function(ctx)
        -- ${(args as { description: string }).description}
        log("Hello from the AI draft")
    end
}
` };
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
          schedule: "0 0 9 * * Mon", run_on_startup: false, watch_path: null, watch_pattern: null, app_version: "1.4.0" },
        risks: ["deletes_files", "opens_apps", "runs_on_schedule"], problems: [] };
      case "import_automation": {
        const a: AutomationSummary = { id: automations.length + 1, name: "Downloads cleaner", description: "", lua_code: "log(1)", schedule: "0 0 9 * * Mon",
          enabled: false, created_at: iso(0), updated_at: iso(0), run_on_startup: false, watch_path: null, watch_pattern: null, last_run: null, next_run: null };
        automations = [...automations, a];
        return a;
      }
      case "dota_get_settings": return { launch_url: "https://www.dotabuff.com/heroes/meta", role: "offlane", gsi_port: 3417, launch_assistant: true,
        account_id: dotaAccount, live_helper: dotaLiveHelper,
        cfg_text: '"LocalFlow Dota 2 companion"\n{\n    "uri"  "http://127.0.0.1:3417/"\n    ...\n}\n' };
      case "dota_status": return { gsi_installed: true, dota_found: true, dota_dir: "C:\\Steam\\steamapps\\common\\dota 2 beta",
        cfg_path: "C:\\Steam\\steamapps\\common\\dota 2 beta\\game\\dota\\cfg\\gamestate_integration\\gamestate_integration_localflow.cfg",
        listening: true, port: 3417, listen_error: null, dota_running: true, in_menu: false, state: "hero_selection", team: null, hero_id: 2,
        hero_name: "npc_dota_hero_axe", last_update: dotaNow, launch_assistant: true, launch_url: "https://www.dotabuff.com/heroes/meta",
        source: { name: "OpenDota", fetched_at: dotaNow - 7200, patch: "7.39", offline: false, note: "" } };
      case "dota_heroes": return dotaHeroes;
      case "dota_draft": case "dota_reset": case "dota_set_hero": case "dota_correct": return dotaDraft;
      case "dota_set_team": dotaDraft = { ...dotaDraft, team: args.team, team_assumed: false }; return dotaDraft;
      case "dota_capture": return { draft: dotaDraft, recognized: 5, layout: "16:9 top bar", width: 1920, height: 1080, warnings: [], image: "capture.png" };
      case "dota_suggest": return [
        { hero_id: 129, hero: "Mars", score: 0.8, reasons: [
          { text: "Strong against Phantom Assassin", evidence: dotaSourced("54.1% win rate over 3,812 games") },
          { text: "Your team has no initiator yet", evidence: { kind: "heuristic", rule: "every draft wants a way to start fights" } }] },
        { hero_id: 26, hero: "Lion", score: 0.6, reasons: [{ text: "Instant disable against Pudge", evidence: { kind: "heuristic", rule: "hex stops channelled and melee heroes" } }] },
      ];
      case "dota_build": return { hero_id: 2, hero: "Axe", data_note: "OpenDota, fetched 2 h ago",
        starting: [{ item: "Tango", key: "tango", priority: 1, why: "Lane sustain", evidence: dotaSourced("bought in 93% of games"), alternatives: [] }],
        core: [{ item: "Blink Dagger", key: "blink", priority: 1, why: "Starts fights with Berserker's Call", evidence: dotaSourced("bought in 88% of games"), alternatives: [] },
          { item: "Blade Mail", key: "blade_mail", priority: 2, why: "Punishes focus", evidence: { kind: "heuristic", rule: "damage return fits a tank" }, alternatives: ["Crimson Guard"] }],
        situational: [{ item: "Black King Bar", key: "black_king_bar", priority: 3, why: "Lots of disables in the enemy draft", evidence: { kind: "heuristic", rule: "magic immunity against disables" }, alternatives: ["Linken's Sphere"] }],
        adaptations: [{ text: "Phantom Assassin: consider Heaven's Halberd", evidence: { kind: "heuristic", rule: "disarm stops physical carries" } }] };
      case "dota_set_settings": case "dota_set_role": case "open_dota": return null;
      case "dota_install_gsi": throw { kind: "error", message: "Could not write C:\\Steam\\...\\gamestate_integration_localflow.cfg (access denied). Create that file yourself and paste the text shown below into it." };
      case "dota_uninstall_gsi": return true;
      case "dota_live": return { hero: "Axe", next_note: null,
        state: { clock: 734, gold: 1840, items: ["tango", "vanguard"], hero_id: 2, alive: true, updated_at: dotaNow },
        next_item: { advice: { item: "Blink Dagger", key: "blink", priority: 1, why: "Starts fights with Berserker's Call", evidence: dotaSourced("bought in 88% of games"), alternatives: [] }, missing_gold: 410, affordable: false },
        reminders: [{ clock: 840, text: "Wisdom runes at 14:00", kind: "wisdom" }, { clock: 780, text: "Power rune at 13:00", kind: "rune" }] };
      case "dota_lookup": return { hero_id: 2, hero: "Axe", traits: ["Initiator", "Durable", "Disabler"], data_note: "OpenDota, fetched 2 h ago",
        strong_against: [{ hero_id: 44, hero: "Phantom Assassin", reason: { text: "Counter Helix punishes her", evidence: dotaSourced("54.1% win rate over 3,812 games") } },
          { hero_id: 8, hero: "Juggernaut", reason: { text: "Call interrupts Blade Fury's setup", evidence: { kind: "heuristic", rule: "taunt stops melee carries" } } }],
        weak_against: [{ hero_id: 26, hero: "Lion", reason: { text: "Hex before the Blink", evidence: dotaSourced("46.2% win rate over 2,904 games") } },
          { hero_id: 11, hero: "Shadow Fiend", reason: { text: "Kites and out-damages in lane", evidence: { kind: "heuristic", rule: "ranged mid heroes beat melee without sustain" } } }],
        common_items: [{ item: "Blink Dagger", key: "blink", priority: 1, why: "Core", evidence: dotaSourced("bought in 88% of games"), alternatives: [] },
          { item: "Blade Mail", key: "blade_mail", priority: 2, why: "Mid game", evidence: dotaSourced("bought in 61% of games"), alternatives: [] }] };
      case "dota_last_match":
        if (!dotaAccount) throw { kind: "error", message: "your Dota account isn't set. Paste your Dotabuff or OpenDota profile link in Settings › Dota 2 companion" };
        return { data_note: "OpenDota, match parsed 3 min ago",
          summary: { match_id: 8012345678, hero_id: 2, hero: "Axe", won: true, kills: 9, deaths: 4, assists: 17, gpm: 512, xpm: 640, last_hits: 212, duration_secs: 2531, start_time: dotaNow - 3200, items: ["Blink Dagger", "Blade Mail", "Black King Bar"] },
          benchmarks: [{ metric: "gold per minute", value: 512, percentile: 0.71 }, { metric: "last hits per minute", value: 5.0, percentile: 0.48 }, { metric: "hero damage per minute", value: 610, percentile: 0.22 }],
          notes: [{ text: "Blink Dagger at 13:10 is a little late", evidence: dotaSourced("median 11:45 over 12,000 Axe games") },
            { text: "4 deaths is fine for an initiator", evidence: { kind: "heuristic", rule: "offlaners die more often" } }] };
      case "dota_recent_matches": return [0, 1, 2, 3].map((i) => ({ match_id: 8012345678 - i, hero_id: [2, 129, 26, 2][i], hero: ["Axe", "Mars", "Lion", "Axe"][i], won: i !== 2,
        kills: 9 - i, deaths: 4 + i, assists: 17 - i, gpm: 512 - i * 40, xpm: 640 - i * 30, last_hits: 212, duration_secs: 2531 - i * 200, start_time: dotaNow - 3200 - i * 7200, items: [] }));
      case "dota_parse_account": { const m = String(args.text).match(/\d{3,}/); if (!m) throw { kind: "error", message: `couldn't find a Dota account id in "${args.text}"` }; return Number(m[0]); }
      case "dota_set_account": { const m = String(args.text ?? "").match(/\d{3,}/); dotaAccount = m ? Number(m[0]) : null; return dotaAccount; }
      case "dota_set_live_helper": dotaLiveHelper = !!args.enabled; return null;
      case "links_list": return linkSets;
      case "links_trash": return linkTrash;
      case "links_save": {
        const set = args.set as MockLinkSet;
        linkSets = linkSets.filter((l) => l.name !== args.oldName && l.name !== set.name).concat([{ ...set, updated_at: Date.now() / 1000 }]);
        return set;
      }
      case "links_delete": { const gone = linkSets.find((l) => l.name === args.name); linkSets = linkSets.filter((l) => l.name !== args.name); if (gone) linkTrash = [gone, ...linkTrash]; return !!gone; }
      case "links_restore": { const back = linkTrash.find((l) => l.name === args.name); linkTrash = linkTrash.filter((l) => l !== back); if (back) linkSets = [...linkSets, back]; return null; }
      case "links_open": return linkSets.find((l) => l.name === args.name)?.links.length ?? 0;
      case "links_parse": return String(args.text).split(/\r?\n/).map((l) => l.match(/https?:\/\/\S+/)?.[0]).filter(Boolean).map((url) => ({ url, title: "" }));
      case "links_import_bookmarks": throw { kind: "error", message: `no bookmarks folder called "${args.folder}"` };
      case "phone_status": return { enabled: true, online: true, relay: "wss://relay.example", phones: [{ device: "d1", name: "Pixel 8", permissions: ["status", "view", "run", "share"], added_at: 1790000000, connected: true }], pending: [] };
      case "phone_pair": return "lfremote://pair?v=1&relay=wss%3A%2F%2Frelay.example&pc=AAAAAAAAAAAAAAAAAAAAAA&key=preview&s=preview&name=Preview";
      default: return null;
    }
  }, { shouldMockEvents: true });
}
