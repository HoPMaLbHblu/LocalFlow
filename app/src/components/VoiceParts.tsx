import { useEffect, useId, useState } from "react";
import type { DownloadProgress, EngineView } from "../api";
import { t } from "../i18n";
import { IS_MAC } from "../i18n/mac";
import { comboFromEvent, downloadErrorKind, formatBytes, pushKeyProblem } from "../voiceUtil";

interface ModelRowProps {
  engine: EngineView;
  progress?: DownloadProgress;
  busy?: boolean;
  onDownload: () => void;
  onCancel: () => void;
  onRemove?: () => void;
}

/** One speech model: what it is, its size and licence, and download / cancel / retry / remove. */
export function ModelRow({ engine, progress, busy, onDownload, onCancel, onRemove }: ModelRowProps) {
  const { info, status } = engine;
  const downloading = !!progress && !progress.finished;
  const failed = !!progress && progress.finished && !!progress.error;
  const kind = failed ? downloadErrorKind(progress!.error ?? "") : null;
  const percent = progress && progress.total > 0 ? Math.min(100, Math.round((progress.done / progress.total) * 100)) : 0;
  const share = /CC[- ]BY[- ]SA/i.test(info.license);
  return (
    <div className="voice-model">
      <div className="voice-model-head">
        <strong>{info.name}</strong>
        <span className="muted small">
          {info.languages.map((l) => l.toUpperCase()).join(", ")} · {formatBytes(info.download_bytes)} · {t("voice.model.license", { license: info.license })}
        </span>
      </div>
      {info.description && <p className="muted small">{info.description}</p>}
      {share && <p className="muted small">{t("voice.model.attribution")}</p>}
      {downloading && (
        <div className="voice-progress-wrap">
          <div
            className="voice-progress"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent}
            aria-label={t("voice.model.downloading", { name: info.name })}
          >
            <div style={{ width: `${percent}%` }} />
          </div>
          <span className="small" aria-live="off">
            {percent}% · {formatBytes(progress!.done)} / {formatBytes(progress!.total || info.download_bytes)}
          </span>
        </div>
      )}
      {failed && (
        <div className="banner error voice-inline-banner" role="alert">
          {kind === "cancelled" ? t("voice.model.cancelled") : kind === "offline" ? t("voice.model.offline") : t("voice.model.failed")}
          {kind !== "cancelled" && progress?.error ? <span className="block small">{progress.error}</span> : null}
        </div>
      )}
      <div className="actions">
        {status.installed && !downloading && (
          <>
            <span className="voice-ok">✓ {t("voice.model.installed", { size: formatBytes(status.size_bytes) })}</span>
            {onRemove && (
              <button className="link small" disabled={busy} onClick={onRemove}>
                {t("voice.model.remove")}
              </button>
            )}
          </>
        )}
        {!status.installed && !downloading && (
          <button className="primary" disabled={busy} onClick={onDownload}>
            {failed ? t("voice.model.retry") : t("voice.model.download", { size: formatBytes(info.download_bytes) })}
          </button>
        )}
        {downloading && (
          <button className="secondary" onClick={onCancel}>
            {t("voice.model.cancel")}
          </button>
        )}
      </div>
    </div>
  );
}

interface KeyFieldProps {
  value: string;
  onChange: (value: string) => void;
  /** An error from the backend (for example a conflict with another shortcut). */
  backendError?: string | null;
  disabled?: boolean;
}

/** The push-to-talk key: typed or recorded, validated, with backend conflicts shown under it. */
export function PushKeyField({ value, onChange, backendError, disabled }: KeyFieldProps) {
  const id = useId();
  const [recording, setRecording] = useState(false);
  const problem = pushKeyProblem(value);

  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setRecording(false);
        return;
      }
      const combo = comboFromEvent(e, IS_MAC);
      if (combo) {
        onChange(combo);
        setRecording(false);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [recording, onChange]);

  const message = problem === "empty" ? t("voice.key.empty") : problem === "invalid" ? t("voice.key.invalid") : backendError;
  return (
    <div className="voice-key">
      <label htmlFor={id}>{t("voice.key.label")}</label>
      <div className="inline-form">
        <input
          id={id}
          className={message ? "invalid" : ""}
          value={recording ? "" : value}
          placeholder={recording ? t("voice.key.recording") : "Ctrl+Alt+Space"}
          disabled={disabled}
          spellCheck={false}
          aria-invalid={!!message}
          aria-describedby={`${id}-help`}
          onChange={(e) => onChange(e.target.value)}
        />
        <button type="button" className="secondary" disabled={disabled} onClick={() => setRecording((r) => !r)}>
          {recording ? t("voice.key.cancelRecord") : t("voice.key.record")}
        </button>
      </div>
      <span id={`${id}-help`} className={`small ${message ? "voice-field-error" : "muted"}`} role={message ? "alert" : undefined}>
        {message ?? t("voice.key.help")}
      </span>
    </div>
  );
}
