import { useMemo, useState } from "react";
import { apiDocs, snippets } from "../guide/content";
import { renderInline } from "../guide/GuideText";
import { t } from "../i18n";

interface Props {
  onInsert: (code: string) => void;
  onOpenGuide: () => void;
  onClose: () => void;
}

/** Side panel next to the editor: function reference and snippets to insert at the cursor. */
export default function HelpPanel({ onInsert, onOpenGuide, onClose }: Props) {
  const [tab, setTab] = useState<"functions" | "snippets">("snippets");
  const [open, setOpen] = useState<string | null>(null);
  const docs = useMemo(apiDocs, []);
  const snippetList = useMemo(snippets, []);

  return (
    <aside className="help-panel">
      <div className="help-panel-header">
        <div className="segmented">
          <button className={tab === "snippets" ? "active" : ""} onClick={() => setTab("snippets")}>
            {t("help.snippets")}
          </button>
          <button className={tab === "functions" ? "active" : ""} onClick={() => setTab("functions")}>
            {t("help.functions")}
          </button>
        </div>
        <button className="link small push-right" onClick={onClose} title={t("help.close")}>
          ✕
        </button>
      </div>

      <div className="help-panel-body">
        {tab === "snippets" &&
          snippetList.map((s) => (
            <div key={s.code} className="help-item">
              <div className="help-item-head">
                <strong>{s.title}</strong>
                <button className="small" onClick={() => onInsert(s.code)} title={t("help.insertTitle")}>
                  {t("help.insert")}
                </button>
              </div>
              <span className="muted small">{s.description}</span>
            </div>
          ))}

        {tab === "functions" &&
          docs.map((doc) => (
            <div key={doc.name} className="help-item">
              <button className="help-item-toggle" onClick={() => setOpen(open === doc.name ? null : doc.name)}>
                <code>{doc.signature}</code>
              </button>
              {open === doc.name && (
                <div className="help-item-detail">
                  <p className="small">{renderInline(doc.summary)}</p>
                  {doc.returns && <p className="muted small">{renderInline(t("help.returns", { value: doc.returns }))}</p>}
                  <pre>{doc.example}</pre>
                  <button className="small" onClick={() => onInsert(doc.example + "\n")}>
                    {t("help.insertExample")}
                  </button>
                </div>
              )}
            </div>
          ))}
      </div>

      <div className="help-panel-footer">
        <button className="link small" onClick={onOpenGuide}>
          {t("help.openGuide")}
        </button>
      </div>
    </aside>
  );
}
