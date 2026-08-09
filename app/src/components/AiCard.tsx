import { useEffect, useState } from "react";
import { api, errorMessages, type AiSettings } from "../api";
import { language, t } from "../i18n";

/** Settings › AI: the GigaChat key (stored by the operating system), scope and model. */
export default function AiCard() {
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [key, setKey] = useState("");
  const [scope, setScope] = useState("GIGACHAT_API_PERS");
  const [model, setModel] = useState("GigaChat-2");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ tone: "ok" | "error"; text: string } | null>(null);

  const load = () =>
    api
      .getAiSettings()
      .then((s) => {
        setSettings(s);
        setScope(s.scope);
        setModel(s.model);
      })
      .catch(() => {});
  useEffect(() => {
    load();
  }, []);

  const act = async (action: () => Promise<string | void>, ok?: (result: string | void) => string) => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await action();
      if (ok) setMessage({ tone: "ok", text: ok(result) });
      await load();
    } catch (e) {
      setMessage({ tone: "error", text: errorMessages(e).join(" ") });
    } finally {
      setBusy(false);
    }
  };

  const save = () =>
    act(async () => {
      await api.setAiSettings(key.trim() || null, scope, model);
      setKey("");
    }, () => t("ai.saved"));

  if (!settings) return null;
  const changed = key.trim() !== "" || scope !== settings.scope || model !== settings.model;

  return (
    <section className="card">
      <strong>{t("ai.title")}</strong>
      <p className="muted small">{t("ai.intro")}</p>
      {message && <div className={`banner ${message.tone}`}>{message.text}</div>}

      <div className="ai-grid">
        <label htmlFor="ai-key">
          {t("ai.key")}
          <input
            id="ai-key"
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={key}
            placeholder={settings.configured ? t("ai.keySaved") : t("ai.keyPlaceholder")}
            onChange={(e) => setKey(e.target.value)}
          />
        </label>
        <label htmlFor="ai-scope">
          {t("ai.scope")}
          <select id="ai-scope" value={scope} onChange={(e) => setScope(e.target.value)}>
            {settings.scopes.map((s) => (
              <option key={s} value={s}>
                {t(`ai.scope.${s}` as never)}
              </option>
            ))}
          </select>
        </label>
        <label htmlFor="ai-model">
          {t("ai.model")}
          <select id="ai-model" value={model} onChange={(e) => setModel(e.target.value)}>
            {settings.models.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className="actions">
        <button className="primary" disabled={busy || !changed || (!settings.configured && !key.trim())} onClick={save}>
          {t("ai.save")}
        </button>
        <button
          className="secondary"
          disabled={busy || !settings.configured}
          onClick={() => act(() => api.testAi(language()), (answer) => t("ai.testOk", { answer: String(answer) }))}
        >
          {busy ? t("ai.working") : t("ai.test")}
        </button>
        {settings.cache_entries > 0 && (
          <button
            className="link small"
            disabled={busy}
            onClick={() => act(async () => String(await api.clearAiCache()), (n) => t("ai.cacheCleared", { n: String(n) }))}
          >
            {t("ai.clearCache", { n: settings.cache_entries })}
          </button>
        )}
        {settings.configured && (
          <button className="link small" disabled={busy} onClick={() => act(api.clearAiKey, () => t("ai.removed"))}>
            {t("ai.remove")}
          </button>
        )}
      </div>
      <p className="muted small">{t("ai.help")}</p>
    </section>
  );
}
