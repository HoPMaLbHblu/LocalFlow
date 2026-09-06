import { useEffect, useState } from "react";
import { t } from "../i18n";
import { api, onCoreEvent, type LogEntry } from "../api";
import { formatTime } from "../format";

const LEVELS = ["all", "info", "notify", "error"] as const;

/** Log viewer; new lines appear live while the automation runs. */
export default function LogsTab({ id }: { id: number }) {
  const [logs, setLogs] = useState<LogEntry[] | null>(null);
  const [level, setLevel] = useState<(typeof LEVELS)[number]>("all");

  useEffect(() => {
    api.listLogs(id).then(setLogs);
    let nextId = -1;
    const unlisten = onCoreEvent((event) => {
      if (event.type === "log" && event.automation_id === id) {
        const entry: LogEntry = {
          id: nextId--,
          automation_id: id,
          level: event.level,
          message: event.message,
          created_at: new Date().toISOString(),
        };
        setLogs((l) => [entry, ...(l ?? [])]);
      }
      // Reload once the run is stored, to pick up the final error line and real ids.
      if (event.type === "run_finished" && event.automation_id === id) api.listLogs(id).then(setLogs);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [id]);

  const shown = (logs ?? []).filter((l) => level === "all" || l.level === level);

  return (
    <div className="page">
      <div className="toolbar">
        {LEVELS.map((l) => (
          <button key={l} className={`chip ${level === l ? "active" : ""}`} onClick={() => setLevel(l)}>
            {l === "all" ? t("logs.all") : l}
          </button>
        ))}
        <span className="muted small push-right">{t("logs.live")}</span>
      </div>
      {logs && shown.length === 0 && <p className="muted">{t("logs.none")}</p>}
      <ul className="log-list">
        {shown.map((l) => (
          <li key={l.id} className={`level-${l.level}`}>
            <span className="muted small log-time">{formatTime(l.created_at)}</span>
            <span className="console-level">{l.level}</span>
            <span className="log-message">{l.message}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
