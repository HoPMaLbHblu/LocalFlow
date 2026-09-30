import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorMessages, type AutomationSummary, type CommandExample, type EngineView, type InputDevice, type VoiceSettings } from "../api";
import { t } from "../i18n";
import { engineFor, pushKeyProblem, recognitionLanguage, useDownloads } from "../voiceUtil";
import { takeVoiceSetupRequest, useVoice } from "../useVoice";
import VoiceSetup from "./VoiceSetup";
import { ModelRow, PushKeyField } from "./VoiceParts";
import { StateIcon } from "./VoicePill";
import FoldCard from "./FoldCard";

const GROUPS = ["automation", "alias", "control", "setting"] as const;

/** Settings › Voice control: switch, listening mode, keys, language, microphone, models, aliases, help and a typed test. */
export default function VoiceCard() {
  const { status, settings, log, refresh, setStatus, clearLog } = useVoice();
  const [message, setMessage] = useState<{ tone: "ok" | "error"; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [setupOpen, setSetupOpen] = useState(false);
  const [devices, setDevices] = useState<InputDevice[]>([]);
  const [engines, setEngines] = useState<EngineView[] | null>(null);
  const [engineError, setEngineError] = useState<string | null>(null);
  const [automations, setAutomations] = useState<AutomationSummary[]>([]);
  const [keyDraft, setKeyDraft] = useState<string | null>(null);
  const [keyError, setKeyError] = useState<string | null>(null);
  const [wakeDraft, setWakeDraft] = useState<string | null>(null);
  const [aliasPhrase, setAliasPhrase] = useState("");
  const [aliasTarget, setAliasTarget] = useState<number | "">("");
  const [commands, setCommands] = useState<CommandExample[] | null>(null);
  const [phrase, setPhrase] = useState("");
  const root = useRef<HTMLElement>(null);

  const loadEngines = useCallback(async () => {
    try {
      setEngines(await api.voiceEngines());
      setEngineError(null);
    } catch (e) {
      setEngines([]);
      setEngineError(errorMessages(e).join(" "));
    }
  }, []);
  const loadDevices = useCallback(() => api.voiceListDevices().then(setDevices).catch(() => setDevices([])), []);
  const loadCommands = useCallback(() => api.voiceCommands().then(setCommands).catch(() => setCommands([])), []);
  const { progress, forget } = useDownloads(() => {
    loadEngines();
    refresh();
  });

  useEffect(() => {
    loadEngines();
    loadDevices();
    api.listAutomations().then(setAutomations).catch(() => {});
  }, [loadEngines, loadDevices]);

  // The card starts folded; asking for the setup unfolds it.
  const [cardOpen, setCardOpen] = useState(false);
  // "Set up voice control" in the sidebar opens the setup here.
  useEffect(() => {
    const open = () => {
      if (takeVoiceSetupRequest()) {
        setCardOpen(true);
        setSetupOpen(true);
        root.current?.scrollIntoView?.({ block: "start" });
      }
    };
    open();
    window.addEventListener("localflow:voice-setup", open);
    return () => window.removeEventListener("localflow:voice-setup", open);
  }, []);

  const act = async (action: () => Promise<unknown>, ok?: string) => {
    setBusy(true);
    setMessage(null);
    try {
      await action();
      if (ok) setMessage({ tone: "ok", text: ok });
      await refresh();
      return true;
    } catch (e) {
      setMessage({ tone: "error", text: errorMessages(e).join(" ") });
      await refresh();
      return false;
    } finally {
      setBusy(false);
    }
  };

  if (!settings || !status) return null;

  const save = (patch: Partial<VoiceSettings>, ok?: string) =>
    act(async () => setStatus(await api.voiceSetSettings({ ...settings, ...patch })), ok ?? t("settings.saved"));

  const toggleEnabled = (on: boolean) => {
    if (on && !settings.setup_done) {
      setMessage({ tone: "error", text: t("voice.needSetup") });
      setSetupOpen(true);
      return;
    }
    if (on && !status.model_ready) {
      setMessage({ tone: "error", text: t("voice.needModel") });
      return;
    }
    act(async () => setStatus(await api.voiceSetEnabled(on)));
  };

  const resolved = recognitionLanguage(settings.language);
  const changeLanguage = (language: string) => {
    const target = engines ? engineFor(engines, recognitionLanguage(language)) : undefined;
    save({ language, engine: target?.info.id ?? settings.engine });
  };

  const applyKey = async () => {
    if (keyDraft === null) return;
    setKeyError(null);
    setBusy(true);
    try {
      setStatus(await api.voiceSetSettings({ ...settings, push_key: keyDraft.trim() }));
      setKeyDraft(null);
      setMessage({ tone: "ok", text: t("settings.saved") });
      await refresh();
    } catch (e) {
      setKeyError(errorMessages(e).join(" "));
    } finally {
      setBusy(false);
    }
  };

  const automationName = (id: number) => automations.find((a) => a.id === id)?.name;
  const addAlias = () => {
    const text = aliasPhrase.trim();
    const target = automations.find((a) => a.id === aliasTarget);
    if (!text || !target) return;
    if (settings.aliases.some((a) => a.phrase.toLowerCase() === text.toLowerCase())) {
      setMessage({ tone: "error", text: t("voice.alias.duplicate", { phrase: text }) });
      return;
    }
    save({ aliases: [...settings.aliases, { phrase: text, automation_id: target.id, automation_name: target.name }] }, t("voice.alias.saved")).then((ok) => {
      if (ok) {
        setAliasPhrase("");
        loadCommands();
      }
    });
  };
  const changeAlias = (index: number, id: number) => {
    const target = automations.find((a) => a.id === id);
    if (!target) return;
    save({ aliases: settings.aliases.map((a, i) => (i === index ? { ...a, automation_id: id, automation_name: target.name } : a)) }, t("voice.alias.saved")).then(loadCommands);
  };

  const submit = () => {
    const text = phrase.trim();
    if (!text) return;
    setPhrase("");
    api.voiceSubmitText(text).catch((e) => setMessage({ tone: "error", text: errorMessages(e).join(" ") }));
  };

  const groupTitle = (g: string) => t(`voice.group.${g}` as "voice.group.automation");
  const modelEngine = engines ? engineFor(engines, resolved) : undefined;

  return (
    <FoldCard title={t("voice.title")} className="voice-card" id="voice-card" ref={root} open={cardOpen} onOpenChange={setCardOpen}>
      <p className="muted small">{t("voice.intro")}</p>
      {message && (
        <div className={`banner ${message.tone}`} role={message.tone === "error" ? "alert" : "status"}>
          {message.text}
        </div>
      )}

      <div className="voice-status-line" aria-live="polite">
        <StateIcon state={status.state} />
        <span>
          <strong>{t(`voice.state.${status.state}` as "voice.state.off")}</strong>
          {status.state === "error" && status.message ? `: ${status.message}` : ""}
        </span>
      </div>

      {setupOpen ? (
        <VoiceSetup
          initial={settings}
          onCancel={() => setSetupOpen(false)}
          onFinished={(st) => {
            setStatus(st);
            setSetupOpen(false);
            setMessage({ tone: "ok", text: t("voice.setup.done") });
            refresh();
            loadEngines();
            loadCommands();
          }}
        />
      ) : !settings.setup_done ? (
        <div className="voice-first-run">
          <p>{t("voice.firstRun")}</p>
          <button className="primary" onClick={() => setSetupOpen(true)}>
            {t("voice.setup.start")}
          </button>
        </div>
      ) : null}

      <label className="switch voice-enable">
        <input type="checkbox" checked={settings.enabled} disabled={busy} onChange={(e) => toggleEnabled(e.target.checked)} />
        <span className="switch-track" />
        <span>
          {t("voice.enable")}
          <span className="muted small block">{t("voice.enableHelp")}</span>
        </span>
      </label>

      {status.pending_confirmation && (
        <div className="voice-confirm" role="alertdialog" aria-label={t("voice.pill.confirmTitle")}>
          <strong>{t("voice.pill.confirmTitle")}</strong>
          <p>{status.pending_confirmation}</p>
          <div className="actions">
            <button className="primary" onClick={() => api.voiceAnswer(true).catch(() => {})}>{t("voice.pill.yes")}</button>
            <button className="secondary" onClick={() => api.voiceAnswer(false).catch(() => {})}>{t("voice.pill.no")}</button>
          </div>
        </div>
      )}

      {settings.setup_done && (
        <>
          <h3 className="bots-heading">{t("voice.mode")}</h3>
          <div role="radiogroup" aria-label={t("voice.mode")}>
            <label className="check">
              <input type="radio" name="voice-card-mode" checked={settings.mode === "push_to_talk"} disabled={busy} onChange={() => save({ mode: "push_to_talk" })} />
              <span>
                {t("voice.mode.ptt")} <em className="muted small">({t("voice.mode.recommended")})</em>
                <span className="muted small block">{t("voice.mode.pttHelp")}</span>
              </span>
            </label>
            <label className="check">
              <input type="radio" name="voice-card-mode" checked={settings.mode === "always_on"} disabled={busy} onChange={() => save({ mode: "always_on" })} />
              <span>
                {t("voice.mode.always")}
                <span className="muted small block">{t("voice.mode.alwaysHelp")}</span>
              </span>
            </label>
          </div>
          {settings.mode === "always_on" && (
            <div className="banner warn voice-inline-banner" role="note">
              {t("voice.mode.alwaysWarning")}
            </div>
          )}

          {settings.mode === "push_to_talk" ? (
            <>
              <PushKeyField value={keyDraft ?? settings.push_key} onChange={(v) => { setKeyDraft(v); setKeyError(null); }} backendError={keyError} disabled={busy} />
              {keyDraft !== null && keyDraft !== settings.push_key && (
                <div className="actions">
                  <button className="primary" disabled={busy || pushKeyProblem(keyDraft) !== null} onClick={applyKey}>{t("voice.apply")}</button>
                  <button className="secondary" onClick={() => { setKeyDraft(null); setKeyError(null); }}>{t("settings.undo")}</button>
                </div>
              )}
            </>
          ) : (
            <>
              <label htmlFor="voice-wake">
                {t("voice.wake.label")}
                <input id="voice-wake" value={wakeDraft ?? settings.wake_phrase} spellCheck={false} onChange={(e) => setWakeDraft(e.target.value)} />
                <span className="muted small">
                  {(wakeDraft ?? settings.wake_phrase).trim().split(/\s+/).filter(Boolean).length < 2 ? t("voice.wake.shortWarning") : t("voice.wake.help")}
                </span>
              </label>
              {wakeDraft !== null && wakeDraft !== settings.wake_phrase && (
                <div className="actions">
                  <button className="primary" disabled={busy || !wakeDraft.trim()} onClick={() => save({ wake_phrase: wakeDraft.trim() }).then(() => setWakeDraft(null))}>{t("voice.apply")}</button>
                  <button className="secondary" onClick={() => setWakeDraft(null)}>{t("settings.undo")}</button>
                </div>
              )}
            </>
          )}

          <div className="setting-row">
            <label htmlFor="voice-language">{t("voice.language")}</label>
            <select id="voice-language" value={settings.language} disabled={busy} onChange={(e) => changeLanguage(e.target.value)}>
              <option value="auto">{t("voice.language.auto", { language: t(`voice.language.${recognitionLanguage("auto")}` as "voice.language.en") })}</option>
              <option value="en">{t("voice.language.en")}</option>
              <option value="ru">{t("voice.language.ru")}</option>
              <option value="de">{t("voice.language.de")}</option>
            </select>
          </div>
          {!status.model_ready && <div className="banner warn voice-inline-banner">{t("voice.needModel")}</div>}

          <div className="setting-row">
            <label htmlFor="voice-mic">{t("voice.microphone")}</label>
            <span className="voice-mic-row">
              <select id="voice-mic" value={settings.microphone ?? ""} disabled={busy} onChange={(e) => save({ microphone: e.target.value || null })}>
                <option value="">{t("voice.microphone.default")}</option>
                {settings.microphone && !devices.some((d) => d.name === settings.microphone) && (
                  <option value={settings.microphone}>{t("voice.microphone.missing", { name: settings.microphone })}</option>
                )}
                {devices.map((d) => (
                  <option key={d.name} value={d.name}>
                    {d.name}
                    {d.is_default ? ` (${t("voice.microphone.defaultTag")})` : ""}
                  </option>
                ))}
              </select>
              <button className="secondary" onClick={loadDevices} aria-label={t("voice.refresh")} title={t("voice.refresh")}>
                ↻
              </button>
            </span>
          </div>
          {devices.length === 0 && <p className="small voice-field-error">{t("voice.devices.none")}</p>}

          <label className="check">
            <input type="checkbox" checked={settings.spoken_feedback} disabled={busy} onChange={(e) => save({ spoken_feedback: e.target.checked })} />
            <span>
              {t("voice.spoken")}
              <span className="muted small block">{t("voice.spokenHelp")}</span>
            </span>
          </label>
          <label className="check">
            <input type="checkbox" checked={settings.run_system_automations} disabled={busy} onChange={(e) => save({ run_system_automations: e.target.checked })} />
            <span>
              {t("voice.runSystem")}
              <span className="muted small block">{t("voice.runSystemHelp")}</span>
            </span>
          </label>
          <label className="check">
            <input type="checkbox" checked={settings.change_settings} disabled={busy} onChange={(e) => save({ change_settings: e.target.checked })} />
            <span>
              {t("voice.changeSettings")}
              <span className="muted small block">{t("voice.changeSettingsHelp")}</span>
            </span>
          </label>

          <h3 className="bots-heading">{t("voice.model.title")}</h3>
          <p className="muted small">{t("voice.model.text")}</p>
          {engineError && (
            <div className="banner error voice-inline-banner" role="alert">
              {t("voice.engine.unavailable")}
              <span className="block small">{engineError}</span>
              <button className="secondary small" onClick={loadEngines}>{t("voice.refresh")}</button>
            </div>
          )}
          {engines?.map((e) => (
            <ModelRow
              key={e.info.id}
              engine={e}
              progress={progress[e.info.id]}
              busy={busy}
              onDownload={() => {
                forget(e.info.id);
                api.voiceDownloadModel(e.info.id).catch((err) => setMessage({ tone: "error", text: errorMessages(err).join(" ") }));
              }}
              onCancel={() => api.voiceCancelDownload(e.info.id).catch(() => {})}
              onRemove={() => act(async () => { await api.voiceRemoveModel(e.info.id); await loadEngines(); }, t("voice.model.removed"))}
            />
          ))}
          {modelEngine && <p className="muted small">{t("voice.model.current", { name: modelEngine.info.name })}</p>}

          <h3 className="bots-heading">{t("voice.alias.title")}</h3>
          <p className="muted small">{t("voice.alias.text")}</p>
          {settings.aliases.length > 0 && (
            <ul className="voice-aliases">
              {settings.aliases.map((a, i) => {
                const exists = automationName(a.automation_id) !== undefined;
                return (
                  <li key={`${a.phrase}-${i}`} className={exists ? "" : "stale"}>
                    <span className="voice-alias-phrase">“{a.phrase}”</span>
                    <span aria-hidden="true">→</span>
                    <select aria-label={t("voice.alias.target", { phrase: a.phrase })} value={exists ? a.automation_id : ""} onChange={(e) => changeAlias(i, Number(e.target.value))}>
                      {!exists && <option value="">{t("voice.alias.missingOption", { name: a.automation_name })}</option>}
                      {automations.map((x) => (
                        <option key={x.id} value={x.id}>{x.name}</option>
                      ))}
                    </select>
                    <button className="link small" disabled={busy} onClick={() => save({ aliases: settings.aliases.filter((_, j) => j !== i) }, t("voice.alias.saved")).then(loadCommands)}>
                      {t("settings.remove")}
                    </button>
                    {!exists && <span className="small voice-field-error block" role="alert">{t("voice.alias.stale", { name: a.automation_name })}</span>}
                  </li>
                );
              })}
            </ul>
          )}
          <form
            className="voice-alias-form"
            onSubmit={(e) => {
              e.preventDefault();
              addAlias();
            }}
          >
            <input value={aliasPhrase} placeholder={t("voice.alias.phrase")} aria-label={t("voice.alias.phrase")} onChange={(e) => setAliasPhrase(e.target.value)} />
            <select aria-label={t("voice.alias.pick")} value={aliasTarget} onChange={(e) => setAliasTarget(e.target.value ? Number(e.target.value) : "")}>
              <option value="">{t("voice.alias.pick")}</option>
              {automations.map((x) => (
                <option key={x.id} value={x.id}>{x.name}</option>
              ))}
            </select>
            <button className="secondary" type="submit" disabled={busy || !aliasPhrase.trim() || aliasTarget === ""}>
              {t("settings.add")}
            </button>
          </form>

          <details className="voice-commands" onToggle={(e) => (e.currentTarget as HTMLDetailsElement).open && loadCommands()}>
            <summary>{t("voice.commands.title")}</summary>
            {commands === null ? (
              <p className="muted small">{t("runs.loading")}</p>
            ) : (
              <>
                {GROUPS.map((g) => {
                  const items = commands.filter((c) => c.group === g);
                  if (items.length === 0) return null;
                  return (
                    <div key={g}>
                      <h4>{groupTitle(g)}</h4>
                      <ul>
                        {items.map((c, i) => (
                          <li key={`${c.say}-${i}`}>
                            <strong>“{c.say}”</strong> <span className="muted small">{c.does}</span>
                          </li>
                        ))}
                      </ul>
                    </div>
                  );
                })}
                <button className="link small" onClick={loadCommands}>{t("voice.refresh")}</button>
              </>
            )}
          </details>

          <h3 className="bots-heading">{t("voice.try.title")}</h3>
          <p className="muted small">{t("voice.try.text")}</p>
          <form className="inline-form" onSubmit={(e) => { e.preventDefault(); submit(); }}>
            <input value={phrase} placeholder={t("voice.try.placeholder")} aria-label={t("voice.try.title")} onChange={(e) => setPhrase(e.target.value)} />
            <button className="secondary" type="submit" disabled={!phrase.trim() || !settings.enabled}>
              {t("voice.try.send")}
            </button>
          </form>
          {!settings.enabled && <p className="small muted">{t("voice.try.off")}</p>}
          {log.length > 0 && (
            <div className="voice-log" aria-live="polite">
              <ul>
                {log.map((e) => (
                  <li key={e.id} className={e.kind === "reply" ? `reply ${e.reply ?? ""}` : "heard"}>
                    <span className="muted small">{e.kind === "heard" ? t("voice.pill.heard") : t("voice.pill.reply")}</span> {e.text}
                  </li>
                ))}
              </ul>
              <button className="link small" onClick={clearLog}>{t("voice.try.clear")}</button>
            </div>
          )}
        </>
      )}

      {settings.setup_done && !setupOpen && (
        <button className="link small" onClick={() => setSetupOpen(true)}>{t("voice.setup.again")}</button>
      )}

      <h3 className="bots-heading">{t("voice.privacy.title")}</h3>
      <p className="muted small">{t("voice.privacy")}</p>
      <p className="muted small">{t("voice.privacy.download")}</p>
      <h3 className="bots-heading">{t("voice.limits.title")}</h3>
      <ul className="muted small voice-bullets">
        <li>{t("voice.limits.accuracy")}</li>
        <li>{t("voice.limits.noise")}</li>
        <li>{t("voice.limits.names")}</li>
        <li>{t("voice.limits.scope")}</li>
        <li>{t("voice.limits.pcSpeech")}</li>
      </ul>
    </FoldCard>
  );
}
