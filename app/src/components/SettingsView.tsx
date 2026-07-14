import { useEffect, useState } from "react";
import { api, errorMessages, type Settings } from "../api";
import { renderInline } from "../guide/GuideText";
import { LANGUAGES, t } from "../i18n";
import { applyTheme, type Theme } from "../theme";

interface Props {
  onLanguageChange: (setting: string) => void;
}

const THEMES: { value: Theme; label: () => string }[] = [
  { value: "system", label: () => t("settings.themeSystem") },
  { value: "light", label: () => t("settings.themeLight") },
  { value: "dark", label: () => t("settings.themeDark") },
];

/** Choices for how long a script may run, in seconds. */
const TIME_LIMITS = [30, 60, 120, 300, 600, 1800, 3600];

function timeLimitLabel(seconds: number): string {
  if (seconds < 60) return t("settings.seconds", { n: seconds });
  if (seconds === 60) return t("settings.oneMinute");
  if (seconds === 3600) return t("settings.oneHour");
  return t("settings.minutes", { n: Math.round(seconds / 60) });
}

export default function SettingsView({ onLanguageChange }: Props) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [dirs, setDirs] = useState<string[]>([]);
  const [newDir, setNewDir] = useState("");
  const [message, setMessage] = useState<{ tone: "ok" | "error"; text: string } | null>(null);

  const load = async () => {
    const s = await api.getSettings();
    setSettings(s);
    setDirs(s.allowed_dirs);
  };

  useEffect(() => {
    load();
  }, []);

  const run = async (action: () => Promise<void>, ok: string) => {
    try {
      await action();
      setMessage({ tone: "ok", text: ok });
      await load();
    } catch (e) {
      setMessage({ tone: "error", text: errorMessages(e).join(" ") });
    }
  };

  const changeTheme = (theme: Theme) => {
    applyTheme(theme); // instant, even before the backend confirms
    setSettings((s) => (s ? { ...s, theme } : s));
    api.setTheme(theme).catch((e) => setMessage({ tone: "error", text: errorMessages(e).join(" ") }));
  };

  const changeLanguage = async (language: string) => {
    try {
      await api.setLanguage(language);
      onLanguageChange(language); // re-renders the whole app in the new language
    } catch (e) {
      setMessage({ tone: "error", text: errorMessages(e).join(" ") });
    }
  };

  if (!settings) return <div className="page muted">{t("runs.loading")}</div>;

  const dirsChanged = dirs.join("\n") !== settings.allowed_dirs.join("\n");

  return (
    <div className="page narrow">
      <h1>{t("settings.title")}</h1>

      {message && <div className={`banner ${message.tone}`}>{message.text}</div>}

      <section className="card">
        <strong>{t("settings.appearance")}</strong>
        <div className="setting-row">
          <span>{t("settings.theme")}</span>
          <div className="segmented" role="radiogroup" aria-label={t("settings.theme")}>
            {THEMES.map((theme) => (
              <button
                key={theme.value}
                role="radio"
                aria-checked={settings.theme === theme.value}
                className={settings.theme === theme.value ? "active" : ""}
                onClick={() => changeTheme(theme.value)}
              >
                {theme.label()}
              </button>
            ))}
          </div>
        </div>
        <div className="setting-row">
          <label htmlFor="language">{t("settings.language")}</label>
          <select id="language" value={settings.language} onChange={(e) => changeLanguage(e.target.value)}>
            <option value="auto">{t("settings.languageAuto")}</option>
            {LANGUAGES.map((l) => (
              <option key={l.code} value={l.code}>
                {l.name}
              </option>
            ))}
          </select>
        </div>
      </section>

      <section className="card setting">
        <div>
          <strong>{t("settings.timeLimit")}</strong>
          <p className="muted small">{t("settings.timeLimitText")}</p>
        </div>
        <select
          value={settings.script_timeout_secs}
          onChange={(e) => run(() => api.setScriptTimeout(Number(e.target.value)), t("settings.saved"))}
        >
          {/* Keep an unusual saved value selectable. */}
          {!TIME_LIMITS.includes(settings.script_timeout_secs) && (
            <option value={settings.script_timeout_secs}>{timeLimitLabel(settings.script_timeout_secs)}</option>
          )}
          {TIME_LIMITS.map((seconds) => (
            <option key={seconds} value={seconds}>
              {timeLimitLabel(seconds)}
            </option>
          ))}
        </select>
      </section>

      <section className="card setting">
        <div>
          <strong>{t("settings.autostart")}</strong>
          <p className="muted small">{t("settings.autostartText")}</p>
        </div>
        <label className="switch">
          <input
            type="checkbox"
            checked={settings.autostart}
            onChange={(e) => run(() => api.setAutostart(e.target.checked), t("settings.saved"))}
          />
          <span className="switch-track" />
        </label>
      </section>

      <section className="card setting">
        <div>
          <strong>{t("settings.notifications")}</strong>
          <p className="muted small">{renderInline(t("settings.notificationsText", { code: "`notify()`" }))}</p>
        </div>
        <label className="switch">
          <input
            type="checkbox"
            checked={settings.notifications}
            onChange={(e) => run(() => api.setNotifications(e.target.checked), t("settings.saved"))}
          />
          <span className="switch-track" />
        </label>
      </section>

      <section className="card">
        <strong>{t("settings.folders")}</strong>
        <p className="muted small">{t("settings.foldersText")}</p>
        <ul className="dir-list">
          {dirs.map((dir) => (
            <li key={dir}>
              <code>{dir}</code>
              <button className="link small" onClick={() => setDirs(dirs.filter((d) => d !== dir))}>
                {t("settings.remove")}
              </button>
            </li>
          ))}
        </ul>
        <form
          className="inline-form"
          onSubmit={(e) => {
            e.preventDefault();
            const value = newDir.trim();
            if (value && !dirs.includes(value)) setDirs([...dirs, value]);
            setNewDir("");
          }}
        >
          <input value={newDir} placeholder={t("settings.folderPlaceholder")} onChange={(e) => setNewDir(e.target.value)} />
          <button className="secondary" type="submit">
            {t("settings.add")}
          </button>
        </form>
        {dirsChanged && (
          <div className="actions">
            <button className="primary" onClick={() => run(() => api.setAllowedDirs(dirs), t("settings.foldersSaved"))}>
              {t("settings.saveFolders")}
            </button>
            <button className="secondary" onClick={() => setDirs(settings.allowed_dirs)}>
              {t("settings.undo")}
            </button>
          </div>
        )}
      </section>

      <section className="card">
        <strong>{t("settings.about")}</strong>
        <p className="muted small">
          {t("settings.version", { version: settings.version })}
          <br />
          {renderInline(t("settings.dataDir", { path: "`" + settings.data_dir + "`" }))}
        </p>
        <p className="muted small">{t("settings.trayNote")}</p>
      </section>
    </div>
  );
}
