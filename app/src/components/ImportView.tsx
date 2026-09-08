import { useEffect, useState } from "react";
import { api, errorMessages, type ImportPreview, type Risk } from "../api";
import { describeTriggers } from "../format";
import { t, translateBackendMessage, type Key } from "../i18n";

interface Props {
  path: string;
  onImported: (id: number) => void;
  onCancel: () => void;
}

/** The more dangerous risks are listed first and highlighted. */
const RISKS: { risk: Risk; key: Key; serious: boolean }[] = [
  { risk: "deletes_files", key: "risk.deletes_files", serious: true },
  { risk: "uses_internet", key: "risk.uses_internet", serious: true },
  { risk: "opens_apps", key: "risk.opens_apps", serious: true },
  { risk: "moves_files", key: "risk.moves_files", serious: false },
  { risk: "writes_files", key: "risk.writes_files", serious: false },
  { risk: "uses_clipboard", key: "risk.uses_clipboard", serious: false },
  { risk: "runs_on_startup", key: "risk.runs_on_startup", serious: false },
  { risk: "watches_folder", key: "risk.watches_folder", serious: false },
  { risk: "runs_on_schedule", key: "risk.runs_on_schedule", serious: false },
];

/** Shows what a .localflow file contains before anything is saved. */
export default function ImportView({ path, onImported, onCancel }: Props) {
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .previewImport(path)
      .then(setPreview)
      .catch((e) => setError(errorMessages(e).join(" ")));
  }, [path]);

  const doImport = async () => {
    setBusy(true);
    try {
      const automation = await api.importAutomation(path);
      onImported(automation.id);
    } catch (e) {
      setError(errorMessages(e).join(" "));
      setBusy(false);
    }
  };

  const fileName = path.split(/[\\/]/).pop() ?? path;

  if (error && !preview) {
    return (
      <div className="page narrow">
        <h1>{t("import.title")}</h1>
        <div className="banner error">{error}</div>
        <button className="secondary" onClick={onCancel}>
          {t("common.cancel")}
        </button>
      </div>
    );
  }
  if (!preview) return <div className="page muted">{t("runs.loading")}</div>;

  const shared = preview.automation;
  const risks = RISKS.filter((r) => preview.risks.includes(r.risk));

  return (
    <div className="page narrow">
      <h1>{t("import.title")}</h1>
      <p className="muted small">
        {t("import.fromFile", { file: fileName })}
        {shared.app_version && ` · ${t("import.madeWith", { version: shared.app_version })}`}
      </p>

      <section className="card">
        <h2>{shared.name}</h2>
        {shared.description && <p>{shared.description}</p>}
        <p className="muted small">
          {t("import.starts")}: {describeTriggers({ ...shared, enabled: true })}
        </p>
      </section>

      <section className="card">
        <h2>{t("import.risksTitle")}</h2>
        {risks.length === 0 ? (
          <p className="ok-text">✓ {t("import.noRisks")}</p>
        ) : (
          <ul className="risk-list">
            {risks.map((r) => (
              <li key={r.risk} className={r.serious ? "serious" : ""}>
                {r.serious ? "⚠️" : "•"} {t(r.key)}
              </li>
            ))}
          </ul>
        )}
        <p className="muted small">{t("import.trustNote")}</p>
      </section>

      {preview.problems.length > 0 && (
        <div className="banner error">
          <strong>{t("import.problems")}</strong>
          {preview.problems.map((p, i) => (
            <div key={i}>{translateBackendMessage(p)}</div>
          ))}
        </div>
      )}

      <section className="card">
        <h2>{t("import.code")}</h2>
        <pre className="import-code">{shared.lua_code}</pre>
      </section>

      {error && <div className="banner error">{error}</div>}

      <div className="actions">
        <button className="primary" onClick={doImport} disabled={busy || preview.problems.length > 0}>
          {t("import.button")}
        </button>
        <button className="secondary" onClick={onCancel}>
          {t("common.cancel")}
        </button>
        <span className="muted small">{t("import.disabledNote")}</span>
      </div>
    </div>
  );
}
