import { useMemo, useState } from "react";
import type { AutomationSummary } from "../api";
import { describeTriggers, formatRelative } from "../format";
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
}

/** Status dot: green/red for the last run, grey if it never ran, hollow if disabled. */
function StatusDot({ automation }: { automation: AutomationSummary }) {
  const status = automation.last_run?.status ?? "never";
  const title = automation.enabled ? `Last run: ${status}` : "Disabled";
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
      <button className="brand" onClick={props.onHome}>
        <img src="/logo.svg" alt="" width={26} height={26} />
        LocalFlow
      </button>

      <button className="primary new-button" onClick={props.onNew}>
        + New automation
      </button>

      {props.automations.length > 5 && (
        <input
          className="search"
          type="search"
          placeholder="Search…"
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
                {a.enabled && a.next_run ? `Next ${formatRelative(a.next_run)}` : describeTriggers(a)}
              </span>
            </span>
          </button>
        ))}
        {props.automations.length === 0 && <p className="muted small pad">No automations yet.</p>}
        {props.automations.length > 0 && filtered.length === 0 && <p className="muted small pad">No matches.</p>}
      </nav>

      <button className={`sidebar-footer ${props.view === "guide" ? "selected" : ""}`} onClick={props.onGuide}>
        📘 Learn
      </button>
      <button className={`sidebar-footer ${props.view === "settings" ? "selected" : ""}`} onClick={props.onSettings}>
        ⚙ Settings
      </button>
    </aside>
  );
}
