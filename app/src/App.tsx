import { useCallback, useEffect, useRef, useState } from "react";
import { api, onCoreEvent, type AutomationSummary, type Template } from "./api";
import Sidebar from "./components/Sidebar";
import Home from "./components/Home";
import AutomationView from "./components/AutomationView";
import TemplatePicker from "./components/TemplatePicker";
import SettingsView from "./components/SettingsView";
import GuidePage from "./components/GuidePage";
import ImportView from "./components/ImportView";
import TrashView from "./components/TrashView";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { onOpenFile } from "./api";
import { t, tMaybe } from "./i18n";

export type View =
  | { kind: "home" }
  | { kind: "new" }
  | { kind: "draft"; template: Template }
  | { kind: "automation"; id: number }
  | { kind: "settings" }
  | { kind: "guide" }
  | { kind: "import"; path: string }
  | { kind: "trash" };

const isLocalflowFile = (path: string) => path.toLowerCase().endsWith(".localflow");

/** Built-in templates come from Rust in English; show them in the current language. */
function localizeTemplate(template: Template): Template {
  return {
    ...template,
    title: tMaybe(`template.${template.slug}.title`) ?? template.title,
    description: tMaybe(`template.${template.slug}.description`) ?? template.description,
  };
}

interface Props {
  /** Where to start, e.g. back on Settings after switching language. */
  initialView?: View;
  onLanguageChange: (setting: string) => void;
}

export default function App({ initialView, onLanguageChange }: Props) {
  const [view, setView] = useState<View>(initialView ?? { kind: "home" });
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
    api.getTemplates().then((list) => setTemplates(list.map(localizeTemplate)));
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
    if (dirty.current && !window.confirm(t("app.unsavedConfirm"))) return;
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

  // .localflow files: opened by double-click (at startup or while running) or dropped on the window.
  useEffect(() => {
    api.takePendingImport().then((path) => path && navigate({ kind: "import", path }));
    const unlistenOpen = onOpenFile((path) => navigate({ kind: "import", path }));
    // Drag and drop is a convenience: if it can't be set up, everything else still works.
    let unlistenDrop: Promise<() => void> = Promise.resolve(() => {});
    try {
      unlistenDrop = getCurrentWebview()
        .onDragDropEvent((event) => {
          if (event.payload.type !== "drop") return;
          const file = event.payload.paths.find(isLocalflowFile);
          if (file) navigate({ kind: "import", path: file });
        })
        .catch(() => () => {});
    } catch {
      // Not running inside the desktop window (browser preview).
    }
    return () => {
      unlistenOpen.then((f) => f());
      unlistenDrop.then((f) => f());
    };
  }, [navigate]);

  // Tell the user once if a backup was restored or a damaged database was repaired.
  const [notice, setNotice] = useState<string | null>(null);
  useEffect(() => {
    api
      .startupNotice()
      .then((n) => {
        if (n?.kind === "restored") setNotice(t("notice.restored", { backup: n.backup }));
        if (n?.kind === "recovered_from_damage") setNotice(t("notice.recovered", { backup: n.backup }));
      })
      .catch(() => {});
  }, []);

  const pickImportFile = async () => {
    const path = await openFileDialog({
      multiple: false,
      directory: false,
      filters: [{ name: t("import.fileType"), extensions: ["localflow"] }],
    });
    if (typeof path === "string") navigate({ kind: "import", path });
  };

  const selectedId = view.kind === "automation" ? view.id : null;
  const openGuide = () => navigate({ kind: "guide" });
  const draftCounter = useRef(0);
  const openDraft = (title: string, code: string) =>
    navigate({
      kind: "draft",
      template: { slug: `guide-${++draftCounter.current}`, title, description: "", schedule: "", code },
    });

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
        onGuide={openGuide}
        onImport={pickImportFile}
        onTrash={() => navigate({ kind: "trash" })}
      />
      <main className="main">
        {notice && (
          <div className="banner ok" onClick={() => setNotice(null)}>
            {notice}
          </div>
        )}
        {loadError && <div className="banner error">{t("app.loadError", { error: loadError })}</div>}
        {view.kind === "home" && (
          <Home
            automations={automations}
            templates={templates}
            onSelect={(id) => navigate({ kind: "automation", id })}
            onTemplate={(template) => navigate({ kind: "draft", template })}
            onNew={() => navigate({ kind: "new" })}
            onGuide={openGuide}
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
            onOpenGuide={openGuide}
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
            onOpenGuide={openGuide}
          />
        )}
        {view.kind === "settings" && <SettingsView onLanguageChange={onLanguageChange} />}
        {view.kind === "guide" && <GuidePage onTry={openDraft} />}
        {view.kind === "trash" && <TrashView onRestored={(id) => navigate({ kind: "automation", id })} />}
        {view.kind === "import" && (
          <ImportView
            key={view.path}
            path={view.path}
            onImported={(id) => navigate({ kind: "automation", id })}
            onCancel={() => navigate({ kind: "home" })}
          />
        )}
      </main>
    </div>
  );
}
