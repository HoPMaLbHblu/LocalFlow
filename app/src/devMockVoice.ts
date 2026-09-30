// Browser-preview fake of the voice-control backend (see devMock.ts). Nothing here touches a
// microphone. Add a query flag to the address to try the error states, e.g. `?voice=error`:
//   ready      setup finished, model installed, voice on (to see the pill and confirmation bar)
//   confirm    (with ready) a confirmation is pending right after the page loads
//   offline    the model download fails as if there were no internet
//   denied     the microphone test fails with "permission denied"
//   nodevice   no input devices at all (and the microphone test says so)
//   silent     the microphone test hears nothing
//   conflict   every push-to-talk key is reported as already taken
//   noengine   the speech engine list is unavailable
//   stateerror voice turns itself into the "error" state shortly after it is switched on
//   error      offline + denied + conflict together
// Flags combine with commas: `?voice=ready,stateerror`.

import { emit } from "@tauri-apps/api/event";
import type {
  CommandExample, DownloadProgress, EngineView, InputDevice, ListenMode, Reply, RunningInfo,
  VoiceEvent, VoiceSettings, VoiceState, VoiceStatus,
} from "./api";

const flags = new Set(
  (new URLSearchParams(location.search).get("voice") ?? "")
    .split(",")
    .flatMap((f) => (f === "error" ? ["offline", "denied", "conflict"] : [f.trim()])),
);
const has = (f: string) => flags.has(f);

const err = (message: string) => ({ kind: "error", message });
const MB = 1024 * 1024;

const ready = has("ready");
let settings: VoiceSettings = {
  setup_done: ready, consented: ready, enabled: ready, mode: "push_to_talk", push_key: "Ctrl+Alt+Space",
  wake_phrase: "hey localflow", language: "auto", microphone: null, engine: ready ? "kroko-en" : "",
  spoken_feedback: false, run_system_automations: false, change_settings: false, aliases: [],
};
let muted = false;
let voiceState: VoiceState = ready ? { state: "idle" } : { state: "off" };
let pending: string | null = null;
// Number of the waiting question, like the real app: a stale answer is ignored.
let questionId = 0;
let running: RunningInfo[] = [];
let nextRun = 100;

let engines: EngineView[] = [
  { info: { id: "kroko-en", name: "Kroko English", description: "Speech recognition for English.", download_bytes: 152 * MB, languages: ["en"], local: true, license: "CC-BY-SA 4.0" },
    status: { engine: "kroko-en", installed: ready, size_bytes: ready ? 152 * MB : 0 }, recommended: true },
  { info: { id: "kroko-de", name: "Kroko Deutsch", description: "Speech recognition for German.", download_bytes: 148 * MB, languages: ["de"], local: true, license: "CC-BY-SA 4.0" },
    status: { engine: "kroko-de", installed: false, size_bytes: 0 }, recommended: true },
  { info: { id: "zipformer-ru", name: "Zipformer Russian", description: "Speech recognition for Russian.", download_bytes: 120 * MB, languages: ["ru"], local: true, license: "Apache-2.0" },
    status: { engine: "zipformer-ru", installed: false, size_bytes: 0 }, recommended: true },
];
const downloads = new Map<string, ReturnType<typeof setInterval>>();

const send = (event: VoiceEvent) => emit("localflow://voice", event).catch(() => {});
const setState = (state: VoiceState) => { voiceState = state; send({ type: "state", state }); };
const idleOrMuted = (): VoiceState => (!settings.enabled ? { state: "off" } : muted ? { state: "muted" } : { state: "idle" });
const reply = (r: Reply) => send({ type: "reply", reply: r });
const LOST_MIC = "The microphone was disconnected. Plug it in again or choose another one in Settings.";

function resolvedLanguage(): "en" | "ru" | "de" {
  if (settings.language === "en" || settings.language === "ru" || settings.language === "de") return settings.language;
  const prefix = navigator.language.split("-")[0];
  return prefix === "ru" || prefix === "de" ? prefix : "en";
}

function modelReady(): boolean {
  return engines.some((e) => e.info.languages.includes(resolvedLanguage()) && e.status.installed);
}

function status(): VoiceStatus {
  return {
    ...voiceState, enabled: settings.enabled, mode: settings.mode as ListenMode, muted, language: resolvedLanguage(),
    model_ready: modelReady(), pending_confirmation: pending, pending_question_id: pending ? questionId : null, running,
  };
}

function startRun(name: string, automationId: number) {
  const run = { run_id: nextRun++, automation_id: automationId, name };
  running = [...running, run];
  setTimeout(() => { running = running.filter((r) => r.run_id !== run.run_id); }, 8000);
}

const AUTOMATIONS = [
  { id: 1, name: "Organize PDF files", risky: false },
  { id: 2, name: "Tidy screenshots", risky: false },
  { id: 3, name: "Back up notes", risky: true },
];

/** The fake "brain": what the real controller does with a sentence. */
function handleText(text: string) {
  const said = text.trim().toLowerCase();
  setState({ state: "processing" });
  send({ type: "heard", text: text.trim(), confidence: 0.93 });
  const finish = (r: Reply) => {
    reply(r);
    if (settings.spoken_feedback && r.speak) {
      setState({ state: "speaking" });
      setTimeout(() => setState(idleOrMuted()), 1200);
    } else setState(idleOrMuted());
  };
  const answer = (yes: boolean) => {
    const asked = pending ?? "";
    pending = null;
    if (!yes) { finish({ kind: "done", text: "Cancelled.", speak: true }); return; }
    const name = /"(.*?)"/.exec(asked)?.[1] ?? "automation";
    startRun(name, AUTOMATIONS.find((a) => a.name === name)?.id ?? 0);
    finish({ kind: "done", text: `Started "${name}".`, speak: true });
  };
  answerFn = answer;
  setTimeout(() => {
    if (pending) {
      const yes = /^(yes|yeah|да|ja)\b/.test(said);
      const no = /^(no|nope|нет|nein|cancel)\b/.test(said);
      if (yes || no) { answer(yes); return; }
    }
    if (/^(stop|stop all|стоп|stopp)\b/.test(said)) {
      const n = running.length;
      running = [];
      finish({ kind: "done", text: n ? `Asked ${n} running automation(s) to stop.` : "Nothing is running.", speak: true });
    } else if (/what can i say|help/.test(said)) {
      finish({ kind: "info", text: "You can say: run Organize PDF files, run Back up notes, stop, mute, what is running.", speak: true });
    } else if (/what.*running/.test(said)) {
      finish({ kind: "info", text: running.length ? "Running: " + running.map((r) => r.name).join(", ") : "Nothing is running.", speak: true });
    } else if (/^(run|start|запусти|starte)\s+/.test(said)) {
      const wanted = said.replace(/^(run|start|запусти|starte)\s+/, "");
      const alias = settings.aliases.find((a) => a.phrase.toLowerCase() === wanted);
      const found = alias ? AUTOMATIONS.find((a) => a.id === alias.automation_id) : AUTOMATIONS.find((a) => a.name.toLowerCase().includes(wanted));
      if (!found) finish({ kind: "problem", text: `I could not find an automation called "${wanted}".`, speak: true });
      else if (found.risky && !(alias || found.name.toLowerCase() === wanted)) {
        finish({ kind: "problem", text: `"${found.name}" can control this PC, so I only run it when you say its exact name.`, speak: true });
      } else if (found.risky) {
        pending = `Run "${found.name}"? It can control this PC. Say yes or no.`;
        reply({ kind: "confirm", text: pending, speak: true });
        send({ type: "confirm", prompt: pending, id: ++questionId });
        setState(idleOrMuted());
      } else if (!alias && found.name.toLowerCase() !== wanted) {
        // A fuzzy match never runs at once: it asks first.
        pending = `Did you mean "${found.name}"? Say yes to run it, or no.`;
        reply({ kind: "confirm", text: pending, speak: true });
        send({ type: "confirm", prompt: pending, id: ++questionId });
        setState(idleOrMuted());
      } else { startRun(found.name, found.id); finish({ kind: "done", text: `Started "${found.name}".`, speak: true }); }
    } else {
      finish({ kind: "problem", text: `I did not understand that. Say "what can I say" for a list.`, speak: true });
    }
  }, 600);
}
let answerFn: ((yes: boolean) => void) | null = null;

function commands(): CommandExample[] {
  const out: CommandExample[] = AUTOMATIONS.map((a) => ({
    say: `Run ${a.name}`, does: a.risky ? "Starts the automation (asks first: it can control this PC)." : "Starts the automation.", group: "automation",
  }));
  for (const a of settings.aliases) out.push({ say: `Run ${a.phrase}`, does: `Starts "${a.automation_name}".`, group: "alias" });
  out.push(
    { say: "Stop", does: "Stops everything that is running.", group: "control" },
    { say: "What is running", does: "Lists the running automations.", group: "control" },
    { say: "What can I say", does: "Reads this list.", group: "control" },
    { say: "Mute", does: "Stops listening until you unmute.", group: "control" },
  );
  if (settings.change_settings) {
    out.push(
      { say: "Turn notifications off", does: "Switches desktop notifications off.", group: "setting" },
      { say: "Dark theme", does: "Changes the theme.", group: "setting" },
      { say: "Turn spoken feedback on", does: "Reads replies aloud.", group: "setting" },
    );
  }
  return out;
}

const MODIFIERS = ["ctrl", "control", "alt", "shift", "cmd", "command", "super", "meta", "win", "option"];

function validate(s: VoiceSettings) {
  const parts = s.push_key.split("+").map((p) => p.trim()).filter(Boolean);
  const last = (parts[parts.length - 1] ?? "").toLowerCase();
  if (!parts.length || MODIFIERS.includes(last)) throw err(`"${s.push_key}" is not a valid shortcut. Use modifiers plus one key, for example Ctrl+Alt+Space.`);
  if (has("conflict") || s.push_key.toLowerCase() === "ctrl+alt+d") {
    throw err(`The shortcut ${s.push_key} is already used by another LocalFlow hotkey (Dota 2: draft helper). Pick a different one.`);
  }
  if (!s.wake_phrase.trim()) throw err("The wake phrase can't be empty.");
  if (s.enabled && !s.consented) throw err("Voice control can't be switched on before you accept the disclosure in the setup.");
}

function afterSwitchedOn() {
  if (has("stateerror")) setTimeout(() => { if (settings.enabled) setState({ state: "error", message: LOST_MIC }); }, 1500);
}

function download(engineId: string) {
  const engine = engines.find((e) => e.info.id === engineId);
  if (!engine) throw err(`Unknown speech model "${engineId}".`);
  if (downloads.has(engineId)) return;
  const total = engine.info.download_bytes;
  let done = 0;
  const tick = (p: Partial<DownloadProgress>) =>
    emit("localflow://voice-download", { engine: engineId, done, total, finished: false, error: null, ...p }).catch(() => {});
  tick({});
  const timer = setInterval(() => {
    if (has("offline") && done > total * 0.25) {
      clearInterval(timer); downloads.delete(engineId);
      tick({ finished: true, error: "Could not download the model: network error (could not reach the download server)." });
      return;
    }
    done = Math.min(total, done + total / 25);
    if (done >= total) {
      clearInterval(timer); downloads.delete(engineId);
      engines = engines.map((e) => (e.info.id === engineId ? { ...e, status: { engine: engineId, installed: true, size_bytes: total } } : e));
      tick({ finished: true });
    } else tick({});
  }, 250);
  downloads.set(engineId, timer);
}

export function voiceMock(cmd: string, args: Record<string, any>): { handled: boolean; value?: unknown } {
  const ok = (value?: unknown) => ({ handled: true, value });
  switch (cmd) {
    case "voice_get_settings": return ok(settings);
    case "voice_set_settings": {
      const next = args.settings as VoiceSettings;
      validate(next);
      const was = settings.enabled;
      if (next.enabled && !was && !engines.some((e) => e.status.installed)) throw err("The speech model is not installed. Download it first.");
      settings = { ...next };
      if (settings.enabled !== was) {
        if (!settings.enabled) { pending = null; muted = false; }
        setState(idleOrMuted());
        if (settings.enabled) afterSwitchedOn();
      }
      return ok(status());
    }
    case "voice_status": return ok(status());
    case "voice_list_devices": {
      const list: InputDevice[] = has("nodevice") ? [] : [
        { name: "Microphone (USB Audio Device)", is_default: true },
        { name: "Headset Microphone (Realtek Audio)", is_default: false },
        { name: "Microphone Array (Intel Smart Sound)", is_default: false },
      ];
      return ok(list);
    }
    case "voice_engines":
      if (has("noengine")) throw err("The speech engine is not available in this build of LocalFlow.");
      return ok(engines);
    case "voice_download_model": download(args.engine); return ok();
    case "voice_cancel_download": {
      const timer = downloads.get(args.engine);
      if (timer) {
        clearInterval(timer); downloads.delete(args.engine);
        emit("localflow://voice-download", { engine: args.engine, done: 0, total: 0, finished: true, error: "Download cancelled." }).catch(() => {});
      }
      return ok();
    }
    case "voice_remove_model":
      engines = engines.map((e) => (e.info.id === args.engine ? { ...e, status: { engine: args.engine, installed: false, size_bytes: 0 } } : e));
      if (!modelReady() && settings.enabled) { settings = { ...settings, enabled: false }; setState({ state: "off" }); }
      return ok();
    case "voice_test_microphone":
      return {
        handled: true,
        value: new Promise((resolve, reject) => setTimeout(() => {
          if (has("nodevice")) reject(err("No microphone was found."));
          else if (has("denied")) reject(err("Microphone access was denied by the system."));
          else if (has("silent")) resolve({ peak: 0.002, rms: 0.0004, heard_sound: false });
          else resolve({ peak: 0.42, rms: 0.07, heard_sound: true });
        }, 1500)),
      };
    case "voice_set_enabled": {
      const on = !!args.enabled;
      if (on) {
        if (!settings.consented || !settings.setup_done) throw err("Finish the voice setup first.");
        if (!modelReady()) throw err("The speech model for this language is not installed. Download it first.");
      }
      settings = { ...settings, enabled: on };
      if (!on) { pending = null; muted = false; }
      setState(idleOrMuted());
      if (on) afterSwitchedOn();
      return ok(status());
    }
    case "voice_set_muted":
      muted = !!args.muted;
      if (settings.enabled) setState(idleOrMuted());
      return ok(status());
    case "voice_press":
      if (settings.enabled && !muted) setState({ state: "listening" });
      return ok();
    case "voice_release":
      if (settings.enabled && !muted && voiceState.state === "listening") {
        setState({ state: "processing" });
        setTimeout(() => handleText("run organize pdf files"), 500);
      }
      return ok();
    case "voice_submit_text":
      if (!settings.enabled) throw err("Voice control is off. Switch it on to try a phrase.");
      handleText(String(args.text));
      return ok();
    case "voice_answer":
      if (pending && answerFn && args.id === questionId) answerFn(!!args.yes);
      return ok();
    case "voice_commands": return ok(commands());
    case "running_automations": return ok(running);
    case "stop_run": { const before = running.length; running = running.filter((r) => r.run_id !== args.runId); return ok(running.length < before); }
    case "stop_all_runs": { const n = running.length; running = []; return ok(n); }
    default: return { handled: false };
  }
}

/** With `?voice=ready,confirm` a confirmation is pending shortly after the page loads. */
if (has("confirm") && ready) {
  pending = 'Run "Back up notes"? It can control this PC. Say yes or no.';
  setTimeout(() => send({ type: "confirm", prompt: pending ?? "", id: ++questionId }), 800);
}
