import { Fragment, useEffect, useState } from "react";
import { api, type AutomationRun } from "../api";
import { formatDuration, formatTime, parseOutput } from "../format";
import StatusBadge from "./StatusBadge";

/** Execution history. `version` changes whenever a new run finishes. */
export default function RunsTab({ id, version }: { id: number; version: number }) {
  const [runs, setRuns] = useState<AutomationRun[] | null>(null);
  const [open, setOpen] = useState<number | null>(null);

  useEffect(() => {
    api.listRuns(id).then(setRuns);
  }, [id, version]);

  if (!runs) return <div className="page muted">Loading…</div>;
  if (runs.length === 0) return <div className="page muted">This automation has not run yet.</div>;

  return (
    <div className="page">
      <table className="table">
        <thead>
          <tr>
            <th>Run</th>
            <th>Status</th>
            <th>Started</th>
            <th>Duration</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {runs.map((run) => (
            <Fragment key={run.id}>
              <tr className="clickable" onClick={() => setOpen(open === run.id ? null : run.id)}>
                <td>#{run.id}</td>
                <td>
                  <StatusBadge status={run.status} />
                </td>
                <td>{formatTime(run.started_at)}</td>
                <td>{formatDuration(run.started_at, run.finished_at)}</td>
                <td className="muted small">{open === run.id ? "▾ hide" : "▸ details"}</td>
              </tr>
              {open === run.id && (
                <tr className="details-row">
                  <td colSpan={5}>
                    {run.error && <pre className="error-box">{run.error}</pre>}
                    {parseOutput(run.output).length > 0 ? (
                      <pre>{run.output}</pre>
                    ) : (
                      !run.error && <p className="muted">No output.</p>
                    )}
                  </td>
                </tr>
              )}
            </Fragment>
          ))}
        </tbody>
      </table>
    </div>
  );
}
