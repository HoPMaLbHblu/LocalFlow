import { useEffect, useRef } from "react";
import { t } from "../i18n";
import type { LogLine } from "../api";
import StatusBadge from "./StatusBadge";
import { explainError } from "../guide/content";
import { renderInline } from "../guide/GuideText";

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
              {t("console.clear")}
            </button>
          </>
        ) : (
          <span className="muted">{t("console.empty")}</span>
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
        {state?.error && explainError(state.error) && (
          <div className="console-hint">💡 {renderInline(explainError(state.error)!)}</div>
        )}
        {state && state.status !== "running" && state.lines.length === 0 && !state.error && (
          <div className="muted">{t("console.noOutput")}</div>
        )}
        <div ref={bottom} />
      </div>
    </div>
  );
}
