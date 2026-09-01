import { useCallback, useEffect, useRef, useState } from "react";
import { api, onCoreEvent, type AutomationSummary, type Template } from "./api";
import Sidebar from "./components/Sidebar";
import Home from "./components/Home";
import AutomationView from "./components/AutomationView";
import TemplatePicker from "./components/TemplatePicker";
import SettingsView from "./components/SettingsView";

export type View =
  | { kind: "home" }
  | { kind: "new" }
  | { kind: "draft"; template: Template }
  | { kind: "automation"; id: number }
  | { kind: "settings" };

export default function App() {
  const [view, setView] = useState<View>({ kind: "home" });
  const [automations, setAutomations] = useState<AutomationSummary[]>([]);
  const [templates, setTemplates] = useState<Template[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  // Set by the editor when there are unsaved changes.
  const dirty = useRef(false);

  const refresh = useCallback(async () => {
    try {
      setAutomations(await api.listAutomations());
      setLoadError(null);
    } catch (e) {
      setLoadError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
    api.getTemplates().then(setTemplates);
    const unlisten = onCoreEvent((event) => {
      if (event.type === "automations_changed" || event.type === "run_finished") refresh();
    });
    // Keep "next run in ..." labels fresh.
    const timer = setInterval(refresh, 30_000);
    return () => {
      unlisten.then((f) => f());
      clearInterval(timer);
    };
  }, [refresh]);

  const navigate = useCallback((next: View) => {
    if (dirty.current && !window.confirm("You have unsaved changes. Discard them?")) return;
    dirty.current = false;
    setView(next);
  }, []);

  const setDirty = useCallback((value: boolean) => {
    dirty.current = value;
  }, []);

  // Warn before the window reloads with unsaved work.
  useEffect(() => {
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      if (dirty.current) e.preventDefault();
    };
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
  }, []);

  const selectedId = view.kind === "automation" ? view.id : null;

  return (
    <div className="app">
      <Sidebar
        automations={automations}
        selectedId={selectedId}
        view={view.kind}
        onSelect={(id) => navigate({ kind: "automation", id })}
        onNew={() => navigate({ kind: "new" })}
        onHome={() => navigate({ kind: "home" })}
        onSettings={() => navigate({ kind: "settings" })}
      />
      <main className="main">
        {loadError && <div className="banner error">Could not load automations: {loadError}</div>}
        {view.kind === "home" && (
          <Home
            automations={automations}
            templates={templates}
            onSelect={(id) => navigate({ kind: "automation", id })}
            onTemplate={(template) => navigate({ kind: "draft", template })}
            onNew={() => navigate({ kind: "new" })}
          />
        )}
        {view.kind === "new" && (
          <TemplatePicker
            templates={templates}
            onPick={(template) => navigate({ kind: "draft", template })}
            onCancel={() => navigate({ kind: "home" })}
          />
        )}
        {view.kind === "draft" && (
          <AutomationView
            key={`draft-${view.template.slug}`}
            id={null}
            template={view.template}
            setDirty={setDirty}
            onSaved={(id) => {
              dirty.current = false;
              setView({ kind: "automation", id });
            }}
            onDeleted={() => navigate({ kind: "home" })}
          />
        )}
        {view.kind === "automation" && (
          <AutomationView
            key={`automation-${view.id}`}
            id={view.id}
            setDirty={setDirty}
            onSaved={() => {}}
            onDeleted={() => {
              dirty.current = false;
              setView({ kind: "home" });
            }}
          />
        )}
        {view.kind === "settings" && <SettingsView />}
      </main>
    </div>
  );
}
