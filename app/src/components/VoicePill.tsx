import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorMessages, type VoiceStateName } from "../api";
import { t } from "../i18n";
import { useVoice } from "../useVoice";

/** A small icon per state, so the state never depends on colour alone. */
export function StateIcon({ state }: { state: VoiceStateName }) {
  const common = { width: 16, height: 16, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 2, strokeLinecap: "round" as const, strokeLinejoin: "round" as const, "aria-hidden": true };
  switch (state) {
    case "listening":
      return (
        <svg {...common}>
          <path d="M3 12v0M7 8v8M11 4v16M15 8v8M19 11v2" />
        </svg>
      );
    case "processing":
      return (
        <svg {...common}>
          <circle cx="5" cy="12" r="1.2" />
          <circle cx="12" cy="12" r="1.2" />
          <circle cx="19" cy="12" r="1.2" />
        </svg>
      );
    case "speaking":
      return (
        <svg {...common}>
          <path d="M4 9v6h4l5 4V5L8 9H4z" />
          <path d="M17 9a4 4 0 0 1 0 6" />
        </svg>
      );
    case "muted":
      return (
        <svg {...common}>
          <rect x="9" y="3" width="6" height="11" rx="3" />
          <path d="M5 11a7 7 0 0 0 11 5.7M19 11v1M12 18v3M3 3l18 18" />
        </svg>
      );
    case "error":
      return (
        <svg {...common}>
          <path d="M12 3 2 20h20L12 3z" />
          <path d="M12 10v4M12 17.5v.01" />
        </svg>
      );
    case "off":
      return (
        <svg {...common}>
          <path d="M12 3v8M6.3 6.8a8 8 0 1 0 11.4 0" />
        </svg>
      );
    default:
      return (
        <svg {...common}>
          <rect x="9" y="3" width="6" height="11" rx="3" />
          <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
        </svg>
      );
  }
}

interface Props {
  /** Open Settings with the voice setup. */
  onSetup: () => void;
  /** Open Settings (the voice card). */
  onSettings: () => void;
}

/** The voice status pill in the sidebar: state, push-to-talk, mute, stop, running automations, confirmation. */
export default function VoicePill({ onSetup, onSettings }: Props) {
  const { status, settings, log, setStatus } = useVoice();
  const [pressed, setPressed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const held = useRef(false);

  const fail = (e: unknown) => setError(errorMessages(e).join(" "));

  const press = useCallback(() => {
    if (held.current) return;
    held.current = true;
    setPressed(true);
    api.voicePress().catch(() => {});
  }, []);
  const release = useCallback(() => {
    if (!held.current) return;
    held.current = false;
    setPressed(false);
    api.voiceRelease().catch(() => {});
  }, []);

  // Never leave the microphone open: let go on blur, hidden window and unmount.
  useEffect(() => {
    const onHide = () => {
      if (document.visibilityState === "hidden") release();
    };
    window.addEventListener("blur", release);
    document.addEventListener("visibilitychange", onHide);
    return () => {
      window.removeEventListener("blur", release);
      document.removeEventListener("visibilitychange", onHide);
      release();
    };
  }, [release]);

  if (!status || !settings) return null;

  const call = async (action: () => Promise<unknown>, after?: (s: Awaited<ReturnType<typeof api.voiceStatus>>) => void) => {
    setError(null);
    try {
      const result = (await action()) as Awaited<ReturnType<typeof api.voiceStatus>> | undefined;
      if (result && typeof result === "object" && "state" in result) {
        setStatus(result);
        after?.(result);
      }
    } catch (e) {
      fail(e);
    }
  };

  // Not set up and off: one quiet line.
  if (!settings.setup_done && !status.enabled) {
    return (
      <button className="voice-setup-link" onClick={onSetup}>
        <StateIcon state="idle" />
        <span>{t("voice.pill.setup")}</span>
      </button>
    );
  }

  const stateLabel = t(`voice.state.${status.state}` as "voice.state.off");
  const lastHeard = [...log].reverse().find((e) => e.kind === "heard");
  const lastReply = [...log].reverse().find((e) => e.kind === "reply");

  if (!status.enabled) {
    return (
      <div className="voice-pill off" role="group" aria-label={t("voice.pill.title")}>
        <div className="voice-pill-row">
          <span className="voice-state">
            <StateIcon state="off" />
            <span aria-live="polite">{t("voice.pill.titleOff")}</span>
          </span>
          <button className="secondary small" onClick={() => call(() => api.voiceSetEnabled(true))}>
            {t("voice.pill.turnOn")}
          </button>
        </div>
        {error && <div className="voice-pill-error" role="alert">{error}</div>}
      </div>
    );
  }

  const idleText =
    status.mode === "always_on"
      ? t("voice.pill.idleWake", { phrase: settings.wake_phrase })
      : t("voice.pill.idlePtt", { key: settings.push_key });
  const stateText = status.state === "idle" ? idleText : status.state === "error" && status.message ? status.message : stateLabel;
  const canTalk = status.state !== "muted" && status.state !== "error" && status.state !== "off";

  return (
    <div className={`voice-pill state-${status.state}`} role="group" aria-label={t("voice.pill.title")}>
      <div className="voice-pill-row">
        <span className="voice-state">
          <StateIcon state={status.state} />
          <strong>{stateLabel}</strong>
        </span>
        <span className="voice-pill-buttons">
          <button
            className="secondary small"
            aria-pressed={status.muted}
            onClick={() => call(() => api.voiceSetMuted(!status.muted))}
            title={status.muted ? t("voice.pill.unmute") : t("voice.pill.mute")}
          >
            {status.muted ? t("voice.pill.unmute") : t("voice.pill.mute")}
          </button>
          <button className="danger-outline small" onClick={() => call(() => api.voiceSetEnabled(false))} title={t("voice.pill.stopHelp")}>
            {t("voice.pill.stop")}
          </button>
        </span>
      </div>
      <div className="voice-pill-text small muted" aria-live="polite" aria-atomic="true">
        {stateText}
      </div>
      <div className="voice-pill-mic small muted">{status.mode === "always_on" ? t("voice.pill.micAlways") : t("voice.pill.micPtt")}</div>

      <button
        className={`voice-talk ${pressed ? "active" : ""}`}
        aria-pressed={pressed}
        disabled={!canTalk}
        title={t("voice.pill.talkHelp")}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture?.(e.pointerId);
          press();
        }}
        onPointerUp={release}
        onPointerCancel={release}
        onLostPointerCapture={release}
        onKeyDown={(e) => {
          if ((e.key === " " || e.key === "Enter") && !e.repeat) {
            e.preventDefault();
            press();
          } else if (e.key === " " || e.key === "Enter") e.preventDefault();
        }}
        onKeyUp={(e) => {
          if (e.key === " " || e.key === "Enter") {
            e.preventDefault();
            release();
          }
        }}
        onBlur={release}
        onContextMenu={(e) => e.preventDefault()}
      >
        {pressed ? t("voice.pill.talking") : t("voice.pill.talk")}
      </button>

      {status.pending_confirmation && (
        <div className="voice-confirm" role="alertdialog" aria-label={t("voice.pill.confirmTitle")}>
          <strong>{t("voice.pill.confirmTitle")}</strong>
          <p>{status.pending_confirmation}</p>
          <div className="actions">
            <button className="primary" autoFocus onClick={() => call(() => api.voiceAnswer(true).then(() => undefined))}>
              {t("voice.pill.yes")}
            </button>
            <button className="secondary" onClick={() => call(() => api.voiceAnswer(false).then(() => undefined))}>
              {t("voice.pill.no")}
            </button>
          </div>
          {settings.mode === "always_on" && <p className="small muted">{t("voice.pill.confirmButtonsOnly")}</p>}
        </div>
      )}

      {status.running.length > 0 && (
        <div className="voice-running">
          <div className="voice-pill-row">
            <span className="small muted">{t("voice.pill.running")}</span>
            {status.running.length > 1 && (
              <button className="link small" onClick={() => call(() => api.stopAllRuns().then(() => undefined))}>
                {t("voice.pill.stopAll")}
              </button>
            )}
          </div>
          <ul>
            {status.running.map((r) => (
              <li key={r.run_id}>
                <span className="voice-run-name">{r.name}</span>
                <button className="danger-outline small" onClick={() => call(() => api.stopRun(r.run_id).then(() => undefined))} aria-label={t("voice.pill.stopRun", { name: r.name })}>
                  {t("voice.pill.stopRunShort")}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {(lastHeard || lastReply) && (
        <div className="voice-last small" aria-live="polite">
          {lastHeard && (
            <div>
              <span className="muted">{t("voice.pill.heard")}</span> “{lastHeard.text}”
            </div>
          )}
          {lastReply && (
            <div className={`voice-reply ${lastReply.reply ?? ""}`}>
              <span className="muted">{t("voice.pill.reply")}</span> {lastReply.text}
            </div>
          )}
        </div>
      )}

      {error && <div className="voice-pill-error" role="alert">{error}</div>}
      {status.state === "error" && (
        <button className="link small" onClick={onSettings}>
          {t("voice.pill.fix")}
        </button>
      )}
    </div>
  );
}
