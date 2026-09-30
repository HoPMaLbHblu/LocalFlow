// Shared live view of voice control for the status pill and the Settings card.
// Everything heard or answered is kept in memory only (the last 20 entries) and never stored.

import { useCallback, useEffect, useRef, useState } from "react";
import { api, onCoreEvent, onVoiceEvent, type Reply, type VoiceSettings, type VoiceStatus } from "./api";

export interface LogEntry {
  id: number;
  kind: "heard" | "reply";
  text: string;
  reply?: Reply["kind"];
}

const MAX_LOG = 20;
let logCounter = 0;

/** Ask Settings to open the voice setup (used by the sidebar pill). */
let setupRequested = false;
export function requestVoiceSetup() {
  setupRequested = true;
  window.dispatchEvent(new Event("localflow:voice-setup"));
}
export function takeVoiceSetupRequest(): boolean {
  const was = setupRequested;
  setupRequested = false;
  return was;
}

export function useVoice() {
  const [status, setStatus] = useState<VoiceStatus | null>(null);
  const [settings, setSettings] = useState<VoiceSettings | null>(null);
  const [log, setLog] = useState<LogEntry[]>([]);
  const alive = useRef(true);

  const refresh = useCallback(async () => {
    try {
      const [st, se] = await Promise.all([api.voiceStatus(), api.voiceGetSettings()]);
      if (alive.current) {
        setStatus(st);
        setSettings(se);
      }
    } catch {
      // Outside the desktop app or an older backend: the pill simply stays hidden.
    }
  }, []);

  const refreshRunning = useCallback(async () => {
    try {
      const running = await api.runningAutomations();
      if (alive.current) setStatus((s) => (s ? { ...s, running } : s));
    } catch {
      /* ignore */
    }
  }, []);

  useEffect(() => {
    alive.current = true;
    refresh();
    const push = (entry: Omit<LogEntry, "id">) =>
      setLog((l) => [...l, { ...entry, id: ++logCounter }].slice(-MAX_LOG));
    const unVoice = onVoiceEvent((e) => {
      if (e.type === "state") {
        setStatus((s) => (s ? { ...s, state: e.state.state, message: e.state.message ?? null } : s));
        refreshRunning();
      } else if (e.type === "heard") push({ kind: "heard", text: e.text });
      else if (e.type === "reply") {
        push({ kind: "reply", text: e.reply.text, reply: e.reply.kind });
        refreshRunning();
      } else if (e.type === "confirm") setStatus((s) => (s ? { ...s, pending_confirmation: e.prompt, pending_question_id: e.id } : s));
      else if (e.type === "settings") setSettings(e.settings);
      if (e.type === "reply" && e.reply.kind !== "confirm") setStatus((s) => (s ? { ...s, pending_confirmation: null, pending_question_id: null } : s));
    });
    const unCore = onCoreEvent((e) => {
      if (e.type === "run_started" || e.type === "run_finished") refreshRunning();
    });
    const timer = setInterval(refresh, 15_000);
    return () => {
      alive.current = false;
      clearInterval(timer);
      unVoice.then((f) => f()).catch(() => {});
      unCore.then((f) => f()).catch(() => {});
    };
  }, [refresh, refreshRunning]);

  const clearLog = useCallback(() => setLog([]), []);
  return { status, settings, log, refresh, setStatus, setSettings, clearLog };
}
