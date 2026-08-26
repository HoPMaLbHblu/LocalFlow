import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import App, { type View } from "./App";
import { api } from "./api";
import { cachedLanguageSetting, setLanguage } from "./i18n";
import { applyTheme, cachedTheme } from "./theme";
import { inDesktopApp, isAiChatWindow, isDotaWindow, isGuideWindow, onPrefsChanged } from "./windowing";
import GuideWindow from "./components/GuideWindow";
import AiChatWindow from "./components/AiChatWindow";
import DotaWindow from "./components/DotaWindow";
import "./styles.css";

/**
 * Applies the theme and language, and re-creates the whole app when the language
 * changes so every text is re-rendered in the new language.
 */
function Root() {
  const [lang, setLang] = useState(() => setLanguage(cachedLanguageSetting()));
  const [initialView, setInitialView] = useState<View | undefined>(undefined);

  useEffect(() => {
    // The cached values paint the first frame; the backend has the saved truth.
    const load = () =>
      api
        .getSettings()
        .then((settings) => {
          applyTheme(settings.theme);
          setLang(setLanguage(settings.language));
        })
        .catch(() => {});
    load();
    // The guide window follows theme and language changes made in the app window.
    if (!inDesktopApp() || !(isGuideWindow() || isAiChatWindow() || isDotaWindow())) return;
    const unlisten = onPrefsChanged(load);
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  if (isGuideWindow()) return <GuideWindow key={lang} />;
  if (isAiChatWindow()) return <AiChatWindow key={lang} />;
  if (isDotaWindow()) return <DotaWindow key={lang} />;

  const changeLanguage = (setting: string) => {
    setInitialView({ kind: "settings" });
    setLang(setLanguage(setting));
  };

  return <App key={lang} initialView={initialView} onLanguageChange={changeLanguage} />;
}

async function start() {
  // Outside the desktop app (plain `npm run dev` in a browser), use fake data.
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window && "invoke" in (window as any).__TAURI_INTERNALS__)) {
    const { installDevMock } = await import("./devMock");
    installDevMock();
  }

  applyTheme(cachedTheme());

  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <Root />
    </React.StrictMode>,
  );
}

start();
