import { useEffect, useState } from "react";
import { api, errorMessages, type Settings } from "../api";

export default function SettingsView() {
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

  if (!settings) return <div className="page muted">Loading…</div>;

  const dirsChanged = dirs.join("\n") !== settings.allowed_dirs.join("\n");

  return (
    <div className="page narrow">
      <h1>Settings</h1>

      {message && <div className={`banner ${message.tone}`}>{message.text}</div>}

      <section className="card setting">
        <div>
          <strong>Start with Windows</strong>
          <p className="muted small">LocalFlow starts quietly in the system tray when you sign in, so schedules keep running.</p>
        </div>
        <label className="switch">
          <input
            type="checkbox"
            checked={settings.autostart}
            onChange={(e) => run(() => api.setAutostart(e.target.checked), "Saved.")}
          />
          <span className="switch-track" />
        </label>
      </section>

      <section className="card setting">
        <div>
          <strong>Desktop notifications</strong>
          <p className="muted small">
            Show a notification when a script calls <code>notify()</code> or a scheduled run fails.
          </p>
        </div>
        <label className="switch">
          <input
            type="checkbox"
            checked={settings.notifications}
            onChange={(e) => run(() => api.setNotifications(e.target.checked), "Saved.")}
          />
          <span className="switch-track" />
        </label>
      </section>

      <section className="card">
        <strong>Allowed folders</strong>
        <p className="muted small">Scripts can only read and change files inside these folders (and their subfolders).</p>
        <ul className="dir-list">
          {dirs.map((dir) => (
            <li key={dir}>
              <code>{dir}</code>
              <button className="link small" onClick={() => setDirs(dirs.filter((d) => d !== dir))}>
                Remove
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
          <input value={newDir} placeholder="C:\Users\you\Projects" onChange={(e) => setNewDir(e.target.value)} />
          <button className="secondary" type="submit">
            Add
          </button>
        </form>
        {dirsChanged && (
          <div className="actions">
            <button className="primary" onClick={() => run(() => api.setAllowedDirs(dirs), "Allowed folders saved.")}>
              Save folders
            </button>
            <button className="secondary" onClick={() => setDirs(settings.allowed_dirs)}>
              Undo
            </button>
          </div>
        )}
      </section>

      <section className="card">
        <strong>About</strong>
        <p className="muted small">
          LocalFlow {settings.version}
          <br />
          Data is stored in <code>{settings.data_dir}</code>
        </p>
        <p className="muted small">Closing the window keeps LocalFlow running in the tray. Use Quit in the tray menu to exit.</p>
      </section>
    </div>
  );
}
