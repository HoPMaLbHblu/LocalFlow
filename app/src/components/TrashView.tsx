import { useEffect, useState } from "react";
import { api, errorMessages, type Automation } from "../api";
import { formatRelative } from "../format";
import { t } from "../i18n";
import { confirmAction } from "../confirm";

/** Deleted automations, with Restore and Delete forever. */
export default function TrashView({ onRestored }: { onRestored: (id: number) => void }) {
  const [items, setItems] = useState<Automation[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = () => api.listTrash().then(setItems).catch((e) => setError(errorMessages(e).join(" ")));
  useEffect(() => {
    load();
  }, []);

  const restore = async (id: number) => {
    try {
      await api.restoreAutomation(id);
      onRestored(id);
    } catch (e) {
      setError(errorMessages(e).join(" "));
    }
  };

  const deleteForever = async (a: Automation) => {
    if (!(await confirmAction(t("trash.deleteForeverConfirm", { name: a.name })))) return;
    try {
      await api.deleteForever(a.id);
      load();
    } catch (e) {
      setError(errorMessages(e).join(" "));
    }
  };

  return (
    <div className="page narrow">
      <h1>{t("trash.title")}</h1>
      <p className="muted">{t("trash.intro")}</p>
      {error && <div className="banner error">{error}</div>}
      {items && items.length === 0 && <p className="muted">{t("trash.empty")}</p>}
      {items?.map((a) => (
        <section key={a.id} className="card setting">
          <div>
            <strong>{a.name}</strong>
            <p className="muted small">{t("trash.deletedAt", { time: formatRelative(a.deleted_at) })}</p>
          </div>
          <div className="actions">
            <button className="primary" onClick={() => restore(a.id)}>
              {t("trash.restore")}
            </button>
            <button className="danger-outline" onClick={() => deleteForever(a)}>
              {t("trash.deleteForever")}
            </button>
          </div>
        </section>
      ))}
    </div>
  );
}
