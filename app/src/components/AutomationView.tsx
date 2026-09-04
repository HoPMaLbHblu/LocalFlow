import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  errorMessages,
  onCoreEvent,
  type AutomationDetail,
  type AutomationInput,
  type Template,
} from "../api";
import { describeTriggers, formatMs, formatDuration, formatRelative, parseOutput, SCHEDULE_PRESETS } from "../format";
import CodeEditor from "./CodeEditor";
import Console, { type ConsoleState } from "./Console";
import RunsTab from "./RunsTab";
import LogsTab from "./LogsTab";
import HelpPanel from "./HelpPanel";
import type { EditorView } from "@codemirror/view";

type Tab = "editor" | "history" | "logs";

interface Props {
  /** `null` for a new, unsaved automation. */
  id: number | null;
  template?: Template;
  setDirty: (dirty: boolean) => void;
  onSaved: (id: number) => void;
  onDeleted: () => void;
  onOpenGuide: () => void;
}

function inputFromTemplate(t: Template): AutomationInput {
  return {
    name: t.title,
    description: t.description,
    lua_code: t.code,
    schedule: t.schedule,
    enabled: true,
    run_on_startup: t.run_on_startup ?? false,
    watch_path: t.watch_path ?? "",
    watch_pattern: t.watch_pattern ?? "",
  };
}

function inputFromAutomation(a: AutomationDetail): AutomationInput {
  return {
    name: a.name,
    description: a.description,
    lua_code: a.lua_code,
    schedule: a.schedule ?? "",
    enabled: a.enabled,
    run_on_startup: a.run_on_startup,
    watch_path: a.watch_path ?? "",
    watch_pattern: a.watch_pattern ?? "",
  };
}

function sameInput(a: AutomationInput, b: AutomationInput) {
  return (
    a.name === b.name &&
    a.description === b.description &&
    a.lua_code === b.lua_code &&
    (a.schedule ?? "") === (b.schedule ?? "") &&
    a.enabled === b.enabled &&
    a.run_on_startup === b.run_on_startup &&
    (a.watch_path ?? "") === (b.watch_path ?? "") &&
    (a.watch_pattern ?? "") === (b.watch_pattern ?? "")
  );
}

export default function AutomationView({ id, template, setDirty, onSaved, onDeleted, onOpenGuide }: Props) {
  const [detail, setDetail] = useState<AutomationDetail | null>(null);
  const [form, setForm] = useState<AutomationInput>(() =>
    template
      ? inputFromTemplate(template)
      : { name: "", description: "", lua_code: "", schedule: "", enabled: true, run_on_startup: false, watch_path: "", watch_pattern: "" },
  );
  const [saved, setSaved] = useState<AutomationInput | null>(null);
  const [tab, setTab] = useState<Tab>("editor");
  const [errors, setErrors] = useState<string[]>([]);
  const [scheduleError, setScheduleError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"saving" | "running" | "testing" | null>(null);
  const [console_, setConsole] = useState<ConsoleState | null>(null);
  const [notFound, setNotFound] = useState(false);
  const [historyVersion, setHistoryVersion] = useState(0);
  const [helpOpen, setHelpOpen] = useState(() => {
    try {
      return localStorage.getItem("localflow.helpOpen") !== "false";
    } catch {
      return true;
    }
  });
  const editorView = useRef<EditorView | null>(null);
  const [watchOpen, setWatchOpen] = useState(false);

  const toggleHelp = () => {
    setHelpOpen((open) => {
      try {
        localStorage.setItem("localflow.helpOpen", String(!open));
      } catch {
        // Preference just won't be remembered.
      }
      return !open;
    });
  };

  /** Insert a snippet at the cursor, matching the indentation of the current line. */
  const insertSnippet = (code: string) => {
    const view = editorView.current;
    if (!view) return;
    const { from, to } = view.state.selection.main;
    const line = view.state.doc.lineAt(from);
    const indent = /^\s*/.exec(line.text)![0];
    const text = code
      .trimEnd()
      .split("\n")
      .map((l, i) => (i === 0 || !l ? l : indent + l))
      .join("\n");
    // Blank line: fill it. Start of a line: push the existing code down. Otherwise: start a new line.
    const insert =
      line.text.trim() === ""
        ? text
        : from <= line.from + indent.length
          ? text + "\n" + indent
          : "\n" + indent + text;
    view.dispatch({ changes: { from, to, insert }, selection: { anchor: from + insert.length } });
    view.focus();
  };

  // Which live log lines belong in the console right now.
  const liveTarget = useRef<"test" | number | null>(null);

  const dirty = saved ? !sameInput(form, saved) : true;
  useEffect(() => {
    setDirty(id === null || dirty);
  }, [dirty, id, setDirty]);

  const load = useCallback(async () => {
    if (id === null) return;
    try {
      const d = await api.getAutomation(id);
      setDetail(d);
      return d;
    } catch {
      setNotFound(true);
    }
  }, [id]);

  useEffect(() => {
    load().then((d) => {
      if (d) {
        const input = inputFromAutomation(d);
        setForm(input);
        setSaved(input);
      }
    });
  }, [load]);

  // Stream log lines into the console and refresh when a run finishes.
  useEffect(() => {
    const unlisten = onCoreEvent((event) => {
      if (event.type === "log") {
        const target = liveTarget.current;
        const matches =
          (target === "test" && event.automation_id === null) ||
          (typeof target === "number" && event.automation_id === target);
        if (matches) {
          setConsole((c) => (c ? { ...c, lines: [...c.lines, { level: event.level, message: event.message }] } : c));
        }
      }
      if (event.type === "run_finished" && event.automation_id === id) {
        load();
        setHistoryVersion((v) => v + 1);
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [id, load]);

  // Check the schedule as the user types.
  useEffect(() => {
    const schedule = form.schedule ?? "";
    const timer = setTimeout(() => api.validateSchedule(schedule).then(setScheduleError), 300);
    return () => clearTimeout(timer);
  }, [form.schedule]);

  const update = (patch: Partial<AutomationInput>) => setForm((f) => ({ ...f, ...patch }));

  const save = useCallback(async () => {
    setBusy("saving");
    setErrors([]);
    try {
      if (id === null) {
        const created = await api.createAutomation(form);
        setDirty(false);
        onSaved(created.id);
      } else {
        await api.updateAutomation(id, form);
        setSaved(form);
        await load();
      }
    } catch (e) {
      setErrors(errorMessages(e));
    } finally {
      setBusy(null);
    }
  }, [form, id, load, onSaved, setDirty]);

  const testRun = useCallback(async () => {
    setBusy("testing");
    liveTarget.current = "test";
    setConsole({ title: "Test run", status: "running", lines: [], error: null, duration: null });
    try {
      const result = await api.testRun(form.lua_code, form.name || "Untitled");
      setConsole({
        title: "Test run",
        status: result.success ? "success" : "failed",
        lines: result.logs,
        error: result.error,
        duration: formatMs(result.duration_ms),
      });
    } catch (e) {
      setConsole({ title: "Test run", status: "failed", lines: [], error: errorMessages(e).join("\n"), duration: null });
    } finally {
      liveTarget.current = null;
      setBusy(null);
    }
  }, [form.lua_code, form.name]);

  const runNow = async () => {
    if (id === null) return;
    if (dirty && !window.confirm("Run now uses the saved version. Your unsaved changes won't be included. Continue?")) {
      return;
    }
    setBusy("running");
    liveTarget.current = id;
    setConsole({ title: "Run", status: "running", lines: [], error: null, duration: null });
    try {
      const run = await api.runAutomation(id);
      setConsole({
        title: `Run #${run.id}`,
        status: run.status,
        lines: parseOutput(run.output),
        error: run.error,
        duration: formatDuration(run.started_at, run.finished_at),
      });
    } catch (e) {
      setConsole({ title: "Run", status: "failed", lines: [], error: errorMessages(e).join("\n"), duration: null });
    } finally {
      liveTarget.current = null;
      setBusy(null);
    }
  };

  const toggleEnabled = async () => {
    if (id === null) {
      update({ enabled: !form.enabled });
      return;
    }
    try {
      const updated = await api.setEnabled(id, !form.enabled);
      update({ enabled: updated.enabled });
      setSaved((s) => (s ? { ...s, enabled: updated.enabled } : s));
      await load();
    } catch (e) {
      setErrors(errorMessages(e));
    }
  };

  const remove = async () => {
    if (id === null) return onDeleted();
    if (!window.confirm(`Delete "${detail?.name}" and all of its history?`)) return;
    await api.deleteAutomation(id);
    onDeleted();
  };

  if (notFound) {
    return (
      <div className="page">
        <p className="muted">This automation no longer exists.</p>
      </div>
    );
  }

  const watching = !!form.watch_path || watchOpen;
  const isPreset = SCHEDULE_PRESETS.some((p) => p.value === (form.schedule ?? ""));

  return (
    <div className="automation-view">
      <header className="view-header">
        <div className="view-title">
          <h1>{id === null ? form.name || "New automation" : detail?.name ?? "…"}</h1>
          <span className="muted small">
            {id === null
              ? "Not saved yet"
              : detail
                ? (detail.enabled && detail.next_run ? `Next run ${formatRelative(detail.next_run)} · ` : "") +
                  describeTriggers(detail)
                : ""}
            {id !== null && dirty && " · unsaved changes"}
          </span>
        </div>
        <div className="actions">
          <label className="switch" title={form.enabled ? "Enabled" : "Disabled"}>
            <input type="checkbox" checked={form.enabled} onChange={toggleEnabled} />
            <span className="switch-track" />
            <span className="small">{form.enabled ? "Enabled" : "Disabled"}</span>
          </label>
          {id !== null && (
            <button onClick={runNow} disabled={busy !== null}>
              {busy === "running" ? "Running…" : "▶ Run now"}
            </button>
          )}
          <button className="danger-outline" onClick={remove}>
            {id === null ? "Discard" : "Delete"}
          </button>
        </div>
      </header>

      {id !== null && (
        <nav className="tabs">
          {(["editor", "history", "logs"] as Tab[]).map((t) => (
            <button key={t} className={tab === t ? "active" : ""} onClick={() => setTab(t)}>
              {t === "editor" ? "Editor" : t === "history" ? "Run history" : "Logs"}
            </button>
          ))}
        </nav>
      )}

      {errors.length > 0 && (
        <div className="banner error">
          {errors.map((e, i) => (
            <div key={i}>{e}</div>
          ))}
        </div>
      )}

      {tab === "editor" && (
        <div className="editor-layout">
          <div className="fields">
            <label>
              Name
              <input value={form.name} maxLength={100} onChange={(e) => update({ name: e.target.value })} />
            </label>
            <label className="grow">
              Description
              <input
                value={form.description}
                placeholder="Optional"
                onChange={(e) => update({ description: e.target.value })}
              />
            </label>
            <label>
              Schedule
              <select
                value={isPreset ? form.schedule ?? "" : "custom"}
                onChange={(e) => update({ schedule: e.target.value === "custom" ? form.schedule || "0 0 * * * *" : e.target.value })}
              >
                {SCHEDULE_PRESETS.map((p) => (
                  <option key={p.value} value={p.value}>
                    {p.label}
                  </option>
                ))}
                <option value="custom">Custom…</option>
              </select>
            </label>
            {!isPreset && (
              <label>
                Cron expression
                <input
                  className={`mono ${scheduleError ? "invalid" : ""}`}
                  value={form.schedule ?? ""}
                  placeholder="sec min hour day month weekday"
                  onChange={(e) => update({ schedule: e.target.value })}
                  title={scheduleError ?? "Six fields: second minute hour day month weekday"}
                />
              </label>
            )}
          </div>
          {scheduleError && <div className="field-error">{scheduleError}</div>}

          <div className="triggers">
            <label className="check">
              <input
                type="checkbox"
                checked={form.run_on_startup}
                onChange={(e) => update({ run_on_startup: e.target.checked })}
              />
              <span>
                Run when LocalFlow starts
                <span className="muted small"> (with Settings › Start with Windows, that's every time you sign in)</span>
              </span>
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={watching}
                onChange={(e) => {
                  setWatchOpen(e.target.checked);
                  update(e.target.checked ? { watch_path: form.watch_path || "~/Downloads" } : { watch_path: "", watch_pattern: "" });
                }}
              />
              <span>Run when a new file appears in a folder</span>
            </label>
            {watching && (
              <div className="watch-fields">
                <label className="grow">
                  Folder
                  <input
                    className="mono"
                    value={form.watch_path ?? ""}
                    placeholder="~/Downloads"
                    onChange={(e) => update({ watch_path: e.target.value })}
                  />
                </label>
                <label>
                  Files
                  <input
                    className="mono"
                    value={form.watch_pattern ?? ""}
                    placeholder="* (all files)"
                    onChange={(e) => update({ watch_pattern: e.target.value })}
                  />
                </label>
                <span className="muted small watch-hint">
                  The new file is in <code>ctx.file</code>.
                </span>
              </div>
            )}
          </div>

          <div className="editor-row">
            <div className="editor-area">
              <CodeEditor
                value={form.lua_code}
                onChange={(v) => update({ lua_code: v })}
                onSave={save}
                onTest={testRun}
                onReady={(view) => (editorView.current = view)}
              />
            </div>
            {helpOpen && <HelpPanel onInsert={insertSnippet} onOpenGuide={onOpenGuide} onClose={toggleHelp} />}
          </div>

          <div className="editor-toolbar">
            <button className="secondary" onClick={testRun} disabled={busy !== null} title="Ctrl+Enter">
              {busy === "testing" ? "Testing…" : "Test run"}
            </button>
            <span className="muted small">Test runs don't save anything, but file operations are real.</span>
            {!helpOpen && (
              <button className="link small" onClick={toggleHelp}>
                ? Show help
              </button>
            )}
            <button className="primary push-right" onClick={save} disabled={busy !== null || (!dirty && id !== null)} title="Ctrl+S">
              {busy === "saving" ? "Saving…" : id === null ? "Create automation" : "Save"}
            </button>
          </div>

          <Console state={console_} onClear={() => setConsole(null)} />
        </div>
      )}

      {tab === "history" && id !== null && <RunsTab id={id} version={historyVersion} />}
      {tab === "logs" && id !== null && <LogsTab id={id} />}
    </div>
  );
}
