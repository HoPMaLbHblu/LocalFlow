import { useState } from "react";
import { API_DOCS, SNIPPETS } from "../guide/content";
import { renderInline } from "../guide/GuideText";

interface Props {
  onInsert: (code: string) => void;
  onOpenGuide: () => void;
  onClose: () => void;
}

/** Side panel next to the editor: function reference and snippets to insert at the cursor. */
export default function HelpPanel({ onInsert, onOpenGuide, onClose }: Props) {
  const [tab, setTab] = useState<"functions" | "snippets">("snippets");
  const [open, setOpen] = useState<string | null>(null);

  return (
    <aside className="help-panel">
      <div className="help-panel-header">
        <div className="segmented">
          <button className={tab === "snippets" ? "active" : ""} onClick={() => setTab("snippets")}>
            Snippets
          </button>
          <button className={tab === "functions" ? "active" : ""} onClick={() => setTab("functions")}>
            Functions
          </button>
        </div>
        <button className="link small push-right" onClick={onClose} title="Close help">
          ✕
        </button>
      </div>

      <div className="help-panel-body">
        {tab === "snippets" &&
          SNIPPETS.map((s) => (
            <div key={s.title} className="help-item">
              <div className="help-item-head">
                <strong>{s.title}</strong>
                <button className="small" onClick={() => onInsert(s.code)} title="Insert at the cursor">
                  Insert
                </button>
              </div>
              <span className="muted small">{s.description}</span>
            </div>
          ))}

        {tab === "functions" &&
          API_DOCS.map((doc) => (
            <div key={doc.name} className="help-item">
              <button className="help-item-toggle" onClick={() => setOpen(open === doc.name ? null : doc.name)}>
                <code>{doc.signature}</code>
              </button>
              {open === doc.name && (
                <div className="help-item-detail">
                  <p className="small">{renderInline(doc.summary)}</p>
                  {doc.returns && <p className="muted small">Returns {renderInline(doc.returns)}.</p>}
                  <pre>{doc.example}</pre>
                  <button className="small" onClick={() => onInsert(doc.example + "\n")}>
                    Insert example
                  </button>
                </div>
              )}
            </div>
          ))}
      </div>

      <div className="help-panel-footer">
        <button className="link small" onClick={onOpenGuide}>
          📘 New to Lua? Open the guide
        </button>
      </div>
    </aside>
  );
}
