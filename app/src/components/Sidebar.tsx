import { useMemo, useState } from "react";
import { t } from "../i18n";
import type { AutomationSummary } from "../api";
import { describeTriggers, formatRelative, statusLabel } from "../format";
import type { View } from "../App";

interface Props {
  automations: AutomationSummary[];
  selectedId: number | null;
  view: View["kind"];
  onSelect: (id: number) => void;
  onNew: () => void;
  onHome: () => void;
  onSettings: () => void;
  onGuide: () => void;
  onImport: () => void;
  onTrash: () => void;
  onLinks: () => void;
  canGoBack: boolean;
  canGoForward: boolean;
  onBack: () => void;
  onForward: () => void;
}

/** Status dot: green/red for the last run, grey if it never ran, hollow if disabled. */
function StatusDot({ automation }: { automation: AutomationSummary }) {
  const status = automation.last_run?.status ?? "never";
  const title = automation.enabled ? t("sidebar.lastRun", { status: statusLabel(status) }) : t("trigger.disabled");
  return <span className={`dot dot-${status} ${automation.enabled ? "" : "dot-disabled"}`} title={title} />;
}

export default function Sidebar(props: Props) {
  const [query, setQuery] = useState("");
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? props.automations.filter((a) => a.name.toLowerCase().includes(q)) : props.automations;
  }, [props.automations, query]);

  return (
    <aside className="sidebar">
      <div className="brand-row">
        <button className="brand" onClick={props.onHome}>
          <img src="/logo.svg" alt="" width={26} height={26} />
          LocalFlow
        </button>
        <div className="history-arrows">
          <button
            className="arrow"
            onClick={props.onBack}
            disabled={!props.canGoBack}
            title={t("nav.back")}
            aria-label={t("nav.back")}
          >
            ←
          </button>
          <button
            className="arrow"
            onClick={props.onForward}
            disabled={!props.canGoForward}
            title={t("nav.forward")}
            aria-label={t("nav.forward")}
          >
            →
          </button>
        </div>
      </div>

      <div className="new-row">
        <button className="primary new-button" onClick={props.onNew}>
          {t("sidebar.new")}
        </button>
        <button className="secondary import-button" onClick={props.onImport} title={t("import.title")}>
          {t("sidebar.import")}
        </button>
      </div>

      {props.automations.length > 5 && (
        <input
          className="search"
          type="search"
          placeholder={t("sidebar.search")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      )}

      <nav className="automation-list">
        {filtered.map((a) => (
          <button
            key={a.id}
            className={`automation-item ${props.selectedId === a.id ? "selected" : ""}`}
            onClick={() => props.onSelect(a.id)}
          >
            <StatusDot automation={a} />
            <span className="automation-item-text">
              <span className="automation-item-name">{a.name}</span>
              <span className="automation-item-meta">
                {a.enabled && a.next_run ? t("sidebar.next", { time: formatRelative(a.next_run) }) : describeTriggers(a)}
              </span>
            </span>
          </button>
        ))}
        {props.automations.length === 0 && <p className="muted small pad">{t("sidebar.empty")}</p>}
        {props.automations.length > 0 && filtered.length === 0 && <p className="muted small pad">{t("sidebar.noMatches")}</p>}
      </nav>

      <button className={`sidebar-footer ${props.view === "links" ? "selected" : ""}`} onClick={props.onLinks}>
        {t("sidebar.links")}
      </button>
      <button className={`sidebar-footer ${props.view === "trash" ? "selected" : ""}`} onClick={props.onTrash}>
        {t("sidebar.trash")}
      </button>
      <button className={`sidebar-footer ${props.view === "guide" ? "selected" : ""}`} onClick={props.onGuide}>
        {t("sidebar.learn")}
      </button>
      <button className={`sidebar-footer ${props.view === "settings" ? "selected" : ""}`} onClick={props.onSettings}>
        {t("sidebar.settings")}
      </button>
    </aside>
  );
}
