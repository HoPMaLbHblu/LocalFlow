import { useEffect, useState } from "react";
import { api, errorMessages, type BackupInfo } from "../api";
import { formatTime } from "../format";
import { t, tMaybe } from "../i18n";
import { confirmAction } from "../confirm";

const SHOWN = 12;

function sizeLabel(bytes: number) {
  return bytes < 1024 * 1024 ? `${Math.ceil(bytes / 1024)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** Settings › Backups: list, back up now, restore (restarts the app). */
export default function BackupsCard() {
  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [message, setMessage] = useState<{ tone: "ok" | "error"; text: string } | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () =>
    api
      .listBackups()
      .then((list) => setBackups(list ?? []))
      .catch(() => {});
  useEffect(() => {
    load();
  }, []);

  const act = async (action: () => Promise<unknown>, ok?: string) => {
    setBusy(true);
    try {
      await action();
      if (ok) setMessage({ tone: "ok", text: ok });
      await load();
    } catch (e) {
      setMessage({ tone: "error", text: errorMessages(e).join(" ") });
    } finally {
      setBusy(false);
    }
  };

  const restore = async (b: BackupInfo) => {
    if (!(await confirmAction(t("backups.restoreConfirm", { time: formatTime(b.created_at) })))) return;
    act(() => api.restoreBackup(b.file_name));
  };

  return (
    <section className="card">
      <strong>{t("backups.title")}</strong>
      <p className="muted small">{t("backups.intro")}</p>
      {message && <div className={`banner ${message.tone}`}>{message.text}</div>}
      <div className="actions">
        <button className="primary" disabled={busy} onClick={() => act(api.backupNow, t("backups.created"))}>
          {t("backups.now")}
        </button>
        <button className="secondary" onClick={() => act(api.openBackupsFolder)}>
          {t("backups.openFolder")}
        </button>
      </div>
      {backups.length === 0 ? (
        <p className="muted small">{t("backups.none")}</p>
      ) : (
        <ul className="backup-list">
          {backups.slice(0, SHOWN).map((b) => (
            <li key={b.file_name}>
              <span>
                {formatTime(b.created_at)}{" "}
                <span className="badge">{tMaybe(`backups.kind.${b.kind}`) ?? b.kind}</span>{" "}
                <span className="muted small">{sizeLabel(b.size)}</span>
              </span>
              {b.kind !== "damaged" && (
                <button className="link small" disabled={busy} onClick={() => restore(b)}>
                  {t("backups.restore")}
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
