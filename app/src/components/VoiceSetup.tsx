import { useCallback, useEffect, useState } from "react";
import { api, errorMessages, type EngineView, type InputDevice, type ListenMode, type MicTest, type VoiceSettings, type VoiceStatus } from "../api";
import { t } from "../i18n";
import { engineFor, formatBytes, micErrorKind, pushKeyProblem, recognitionLanguage, useDownloads } from "../voiceUtil";
import { ModelRow, PushKeyField } from "./VoiceParts";

const STEPS = ["consent", "language", "model", "microphone", "mode", "finish"] as const;
type Step = (typeof STEPS)[number];

interface Props {
  initial: VoiceSettings;
  onFinished: (status: VoiceStatus) => void;
  onCancel: () => void;
}

/** First-run voice setup: disclosure, language, model, microphone, listening mode, finish. */
export default function VoiceSetup({ initial, onFinished, onCancel }: Props) {
  const [index, setIndex] = useState(0);
  const step: Step = STEPS[index];
  const [consent, setConsent] = useState(initial.consented);
  const [lang, setLang] = useState(initial.language || "auto");
  const [microphone, setMicrophone] = useState<string | null>(initial.microphone);
  const [mode, setMode] = useState<ListenMode>(initial.mode);
  const [pushKey, setPushKey] = useState(initial.push_key);
  const [wake, setWake] = useState(initial.wake_phrase);
  const [spoken, setSpoken] = useState(initial.spoken_feedback);

  const [engines, setEngines] = useState<EngineView[] | null>(null);
  const [engineError, setEngineError] = useState<string | null>(null);
  const [devices, setDevices] = useState<InputDevice[] | null>(null);
  const [devicesError, setDevicesError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [test, setTest] = useState<{ result?: MicTest; error?: string } | null>(null);
  const [keyError, setKeyError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [downloadStartError, setDownloadStartError] = useState<string | null>(null);

  const loadEngines = useCallback(async () => {
    setEngineError(null);
    try {
      setEngines(await api.voiceEngines());
    } catch (e) {
      setEngines([]);
      setEngineError(errorMessages(e).join(" "));
    }
  }, []);
  const { progress, forget } = useDownloads(() => {
    loadEngines();
  });

  useEffect(() => {
    loadEngines();
  }, [loadEngines]);

  const loadDevices = useCallback(async () => {
    setDevicesError(null);
    try {
      setDevices(await api.voiceListDevices());
    } catch (e) {
      setDevices([]);
      setDevicesError(errorMessages(e).join(" "));
    }
  }, []);
  useEffect(() => {
    if (step === "microphone") loadDevices();
  }, [step, loadDevices]);

  const resolved = recognitionLanguage(lang);
  const engine = engines ? engineFor(engines, resolved) : undefined;
  const modelInstalled = !!engine?.status.installed;

  const startDownload = async () => {
    if (!engine) return;
    setDownloadStartError(null);
    forget(engine.info.id);
    try {
      await api.voiceDownloadModel(engine.info.id);
    } catch (e) {
      setDownloadStartError(errorMessages(e).join(" "));
    }
  };

  const runTest = async () => {
    setTesting(true);
    setTest(null);
    try {
      setTest({ result: await api.voiceTestMicrophone(microphone) });
    } catch (e) {
      setTest({ error: errorMessages(e).join(" ") });
    } finally {
      setTesting(false);
    }
  };

  const modeProblem =
    mode === "push_to_talk" ? pushKeyProblem(pushKey) !== null : wake.trim() === "";

  const canNext =
    step === "consent" ? consent : step === "model" ? modelInstalled : step === "mode" ? !modeProblem : true;

  const finish = async (enable: boolean) => {
    setSaving(true);
    setMessage(null);
    setKeyError(null);
    try {
      const status = await api.voiceSetSettings({
        ...initial,
        setup_done: true,
        consented: true,
        enabled: enable,
        language: lang,
        microphone,
        engine: engine?.info.id ?? initial.engine,
        mode,
        push_key: pushKey.trim(),
        wake_phrase: wake.trim(),
        spoken_feedback: spoken,
      });
      onFinished(status);
    } catch (e) {
      const text = errorMessages(e).join(" ");
      setMessage(text);
      if (/shortcut|hotkey|key/i.test(text) && mode === "push_to_talk") {
        setKeyError(text);
        setIndex(STEPS.indexOf("mode"));
      } else if (/model|download/i.test(text)) setIndex(STEPS.indexOf("model"));
    } finally {
      setSaving(false);
    }
  };

  const micHelp = (kind: "permission" | "nodevice" | "silent" | "other") => <p className="small">{t(`voice.mic.${kind}` as "voice.mic.other")}</p>;

  return (
    <div className="voice-setup" role="region" aria-label={t("voice.setup.title")}>
      <div className="voice-setup-head">
        <strong>{t("voice.setup.title")}</strong>
        <span className="muted small" aria-live="polite">
          {t("voice.setup.step", { n: index + 1, total: STEPS.length })}: {t(`voice.setup.name.${step}` as "voice.setup.name.consent")}
        </span>
      </div>
      <ol className="voice-steps" aria-hidden="true">
        {STEPS.map((s, i) => (
          <li key={s} className={i < index ? "done" : i === index ? "current" : ""} />
        ))}
      </ol>

      {message && (
        <div className="banner error voice-inline-banner" role="alert">
          {message}
        </div>
      )}

      {step === "consent" && (
        <div>
          <p>{t("voice.setup.intro")}</p>
          <ul className="voice-bullets small">
            <li>{t("voice.setup.local")}</li>
            <li>{t("voice.setup.nothingStored")}</li>
            <li>{t("voice.setup.micOnlyWhenOn")}</li>
            <li>{t("voice.setup.download")}</li>
            <li>{t("voice.setup.noCloud")}</li>
          </ul>
          {engines && engines.length > 0 && (
            <ul className="muted small voice-bullets">
              {engines.map((e) => (
                <li key={e.info.id}>
                  {e.info.name}: {formatBytes(e.info.download_bytes)}, {t("voice.model.license", { license: e.info.license })}
                </li>
              ))}
            </ul>
          )}
          {engineError && <div className="banner error voice-inline-banner">{t("voice.engine.unavailable")} {engineError}</div>}
          <label className="check">
            <input type="checkbox" checked={consent} onChange={(e) => setConsent(e.target.checked)} />
            <span>{t("voice.setup.consent")}</span>
          </label>
        </div>
      )}

      {step === "language" && (
        <div role="radiogroup" aria-label={t("voice.language")}>
          <p>{t("voice.setup.languageText")}</p>
          {(["auto", "en", "ru", "de"] as const).map((code) => (
            <label key={code} className="check">
              <input type="radio" name="voice-lang" checked={lang === code} onChange={() => setLang(code)} />
              <span>{code === "auto" ? t("voice.language.auto", { language: t(`voice.language.${recognitionLanguage("auto")}` as "voice.language.en") }) : t(`voice.language.${code}` as "voice.language.en")}</span>
            </label>
          ))}
          <p className="muted small">{t("voice.setup.languageNote")}</p>
        </div>
      )}

      {step === "model" && (
        <div>
          <p>{t("voice.setup.modelText", { language: t(`voice.language.${resolved}` as "voice.language.en") })}</p>
          {engines === null && <p className="muted">{t("runs.loading")}</p>}
          {engineError && (
            <div className="banner error voice-inline-banner" role="alert">
              {t("voice.engine.unavailable")}
              <span className="block small">{engineError}</span>
              <button className="secondary small" onClick={loadEngines}>
                {t("voice.refresh")}
              </button>
            </div>
          )}
          {engines && !engine && !engineError && <div className="banner error voice-inline-banner">{t("voice.engine.noneForLanguage")}</div>}
          {engine && (
            <ModelRow
              engine={engine}
              progress={progress[engine.info.id]}
              onDownload={startDownload}
              onCancel={() => api.voiceCancelDownload(engine.info.id).catch((e) => setDownloadStartError(errorMessages(e).join(" ")))}
            />
          )}
          {downloadStartError && <div className="banner error voice-inline-banner" role="alert">{downloadStartError}</div>}
          <p className="muted small">{t("voice.setup.offlineNote")}</p>
        </div>
      )}

      {step === "microphone" && (
        <div>
          <p>{t("voice.setup.micText")}</p>
          {devices && devices.length === 0 && (
            <div className="banner error voice-inline-banner" role="alert">
              {t("voice.devices.none")}
              {devicesError && <span className="block small">{devicesError}</span>}
            </div>
          )}
          <div className="voice-mic-row">
            <label htmlFor="voice-setup-mic">
              {t("voice.microphone")}
              <select
                id="voice-setup-mic"
                value={microphone ?? ""}
                onChange={(e) => {
                  setMicrophone(e.target.value || null);
                  setTest(null);
                }}
              >
                <option value="">{t("voice.microphone.default")}</option>
                {microphone && !devices?.some((d) => d.name === microphone) && <option value={microphone}>{t("voice.microphone.missing", { name: microphone })}</option>}
                {devices?.map((d) => (
                  <option key={d.name} value={d.name}>
                    {d.name}
                    {d.is_default ? ` (${t("voice.microphone.defaultTag")})` : ""}
                  </option>
                ))}
              </select>
            </label>
            <button className="secondary" onClick={loadDevices}>
              {t("voice.refresh")}
            </button>
          </div>
          <p className="muted small">{t("voice.setup.testText")}</p>
          <div className="actions">
            <button className="primary" disabled={testing || (devices !== null && devices.length === 0)} onClick={runTest}>
              {testing ? t("voice.mic.testing") : t("voice.mic.test")}
            </button>
            {testing && <span className="small" role="status">{t("voice.mic.speakNow")}</span>}
          </div>
          <div aria-live="polite">
            {test?.result?.heard_sound && <div className="banner ok voice-inline-banner">{t("voice.mic.ok")}</div>}
            {test?.result && !test.result.heard_sound && (
              <div className="banner error voice-inline-banner">
                <strong>{t("voice.mic.silentTitle")}</strong>
                {micHelp("silent")}
              </div>
            )}
            {test?.error && (
              <div className="banner error voice-inline-banner" role="alert">
                <strong>{test.error}</strong>
                {micHelp(micErrorKind(test.error) === "other" ? "other" : (micErrorKind(test.error) as "permission" | "nodevice"))}
              </div>
            )}
          </div>
          <p className="muted small">{t("voice.setup.micSkip")}</p>
        </div>
      )}

      {step === "mode" && (
        <div>
          <p>{t("voice.setup.modeText")}</p>
          <div role="radiogroup" aria-label={t("voice.mode")}>
            <label className="check">
              <input type="radio" name="voice-mode" checked={mode === "push_to_talk"} onChange={() => setMode("push_to_talk")} />
              <span>
                {t("voice.mode.ptt")} <em className="muted small">({t("voice.mode.recommended")})</em>
                <span className="muted small block">{t("voice.mode.pttHelp")}</span>
              </span>
            </label>
            <label className="check">
              <input type="radio" name="voice-mode" checked={mode === "always_on"} onChange={() => setMode("always_on")} />
              <span>
                {t("voice.mode.always")}
                <span className="muted small block">{t("voice.mode.alwaysHelp")}</span>
              </span>
            </label>
          </div>
          {mode === "always_on" && (
            <div className="banner warn voice-inline-banner" role="note">
              {t("voice.mode.alwaysWarning")}
            </div>
          )}
          {mode === "push_to_talk" ? (
            <PushKeyField value={pushKey} onChange={(v) => { setPushKey(v); setKeyError(null); }} backendError={keyError} />
          ) : (
            <label htmlFor="voice-setup-wake">
              {t("voice.wake.label")}
              <input id="voice-setup-wake" value={wake} spellCheck={false} onChange={(e) => setWake(e.target.value)} />
              <span className="muted small">{wake.trim().split(/\s+/).filter(Boolean).length < 2 ? t("voice.wake.shortWarning") : t("voice.wake.help")}</span>
            </label>
          )}
          <label className="check">
            <input type="checkbox" checked={spoken} onChange={(e) => setSpoken(e.target.checked)} />
            <span>
              {t("voice.spoken")}
              <span className="muted small block">{t("voice.spokenHelp")}</span>
            </span>
          </label>
        </div>
      )}

      {step === "finish" && (
        <div>
          <p>{t("voice.setup.finishText")}</p>
          <ul className="voice-bullets small">
            <li>{t("voice.setup.sumLanguage", { language: t(`voice.language.${resolved}` as "voice.language.en") })}</li>
            <li>{t("voice.setup.sumMic", { name: microphone ?? t("voice.microphone.default") })}</li>
            <li>{mode === "push_to_talk" ? t("voice.setup.sumPtt", { key: pushKey }) : t("voice.setup.sumWake", { phrase: wake })}</li>
            <li>{spoken ? t("voice.setup.sumSpokenOn") : t("voice.setup.sumSpokenOff")}</li>
            <li>{t("voice.setup.sumSafe")}</li>
          </ul>
          <div className="actions">
            <button className="primary" disabled={saving} onClick={() => finish(true)}>
              {saving ? t("ai.working") : t("voice.setup.finishOn")}
            </button>
            <button className="secondary" disabled={saving} onClick={() => finish(false)}>
              {t("voice.setup.finishOff")}
            </button>
          </div>
        </div>
      )}

      <div className="actions voice-nav">
        {index > 0 && (
          <button className="secondary" onClick={() => { setMessage(null); setIndex(index - 1); }}>
            {t("voice.setup.back")}
          </button>
        )}
        {index < STEPS.length - 1 && (
          <button className="primary" disabled={!canNext} onClick={() => { setMessage(null); setIndex(index + 1); }}>
            {t("voice.setup.next")}
          </button>
        )}
        <button className="link small" onClick={onCancel}>
          {t("voice.setup.cancel")}
        </button>
      </div>
    </div>
  );
}
