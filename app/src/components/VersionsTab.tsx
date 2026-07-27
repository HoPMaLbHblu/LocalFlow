import { useEffect, useState } from "react";
import { api, errorMessages, type AutomationVersion } from "../api";
import { describeTriggers, formatTime } from "../format";
import { t } from "../i18n";
import { confirmAction } from "../confirm";

interface Props {
  id: number;
  /** Changes after every save, so the list stays current. */
  version: number;
  onRestored: () => void;
}

/** Earlier saved versions of an automation, each of which can be put back. */
export default function VersionsTab({ id, version, onRestored }: Props) {
  const [versions, setVersions] = useState<AutomationVersion[] | null>(null);
  const [open, setOpen] = useState<number | null>(null);
  const [message, setMessage] = useState<{ tone: "ok" | "error"; text: string } | null>(null);

  useEffect(() => {
    api.listVersions(id).then(setVersions);
  }, [id, version]);

  const restore = async (v: AutomationVersion) => {
    if (!(await confirmAction(t("versions.restoreConfirm")))) return;
    try {
      await api.restoreVersion(id, v.id);
      setMessage({ tone: "ok", text: t("versions.restored") });
      setVersions(await api.listVersions(id));
      onRestored();
    } catch (e) {
      setMessage({ tone: "error", text: errorMessages(e).join(" ") });
    }
  };

  if (!versions) return <div className="page muted">{t("runs.loading")}</div>;

  return (
    <div className="page">
      <p className="muted">{t("versions.intro")}</p>
      {message && <div className={`banner ${message.tone}`}>{message.text}</div>}
      {versions.length === 0 && <p className="muted">{t("versions.none")}</p>}
      {versions.map((v) => (
        <section key={v.id} className="card version">
          <div className="setting">
            <div>
              <strong>{v.name}</strong>
              <p className="muted small">
                {t("versions.savedAt", { time: formatTime(v.saved_at) })} · {describeTriggers({ ...v, enabled: true })}
              </p>
            </div>
            <div className="actions">
              <button className="secondary" onClick={() => setOpen(open === v.id ? null : v.id)}>
                {open === v.id ? t("versions.hide") : t("versions.show")}
              </button>
              <button onClick={() => restore(v)}>{t("versions.restore")}</button>
            </div>
          </div>
          {open === v.id && <pre>{v.lua_code}</pre>}
        </section>
      ))}
    </div>
  );
}
