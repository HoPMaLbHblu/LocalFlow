import { useEffect, useRef } from "react";
import type { LogLine } from "../api";
import StatusBadge from "./StatusBadge";

export interface ConsoleState {
  title: string;
  status: "running" | "success" | "failed";
  lines: LogLine[];
  error: string | null;
  duration: string | null;
}

interface Props {
  state: ConsoleState | null;
  onClear: () => void;
}

/** Output panel under the editor for test runs and manual runs. */
export default function Console({ state, onClear }: Props) {
  const bottom = useRef<HTMLDivElement>(null);
  useEffect(() => {
    bottom.current?.scrollIntoView({ block: "nearest" });
  }, [state?.lines.length, state?.error]);

  return (
    <div className="console">
      <div className="console-header">
        {state ? (
          <>
            <span>{state.title}</span>
            <StatusBadge status={state.status} />
            {state.duration && <span className="muted small">{state.duration}</span>}
            <button className="link small push-right" onClick={onClear}>
              Clear
            </button>
          </>
        ) : (
          <span className="muted">Output — press Test run (Ctrl+Enter) to try your script without saving.</span>
        )}
      </div>
      <div className="console-body">
        {state?.lines.map((line, i) => (
          <div key={i} className={`console-line level-${line.level}`}>
            <span className="console-level">{line.level}</span>
            <span>{line.message}</span>
          </div>
        ))}
        {state?.error && (
          <div className="console-line level-error">
            <span className="console-level">error</span>
            <span>{state.error}</span>
          </div>
        )}
        {state && state.status !== "running" && state.lines.length === 0 && !state.error && (
          <div className="muted">No output.</div>
        )}
        <div ref={bottom} />
      </div>
    </div>
  );
}
