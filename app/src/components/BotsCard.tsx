import { useEffect, useState } from "react";
import { api, errorMessages, type BotSettings, type FoundChat } from "../api";
import { renderInline } from "../guide/GuideText";
import { t } from "../i18n";

/** Settings › Telegram and Discord: the bot, your chat, remote control, the webhook. */
export default function BotsCard() {
  const [settings, setSettings] = useState<BotSettings | null>(null);
  const [token, setToken] = useState("");
  const [chat, setChat] = useState("");
  const [webhook, setWebhook] = useState("");
  const [remote, setRemote] = useState(false);
  const [power, setPower] = useState(false);
  const [found, setFound] = useState<{ bot: string; chats: FoundChat[] } | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ tone: "ok" | "error"; text: string } | null>(null);

  const load = () =>
    api
      .getBotSettings()
      .then((s) => {
        setSettings(s);
        setChat(s.telegram_chat || "");
        setRemote(s.remote);
        setPower(s.remote_power);
      })
      .catch(() => {});
  useEffect(() => {
    load();
  }, []);

  const act = async (action: () => Promise<unknown>, ok?: (result: unknown) => string) => {
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

  if (!settings) return null;
  const hasTelegram = settings.telegram_token || token.trim() !== "";
  const changed =
    token.trim() !== "" ||
    webhook.trim() !== "" ||
    chat.trim() !== (settings.telegram_chat || "") ||
    remote !== settings.remote ||
    power !== settings.remote_power;

  const save = () =>
    act(async () => {
      await api.setBotSettings(token.trim() || null, chat.trim() || null, webhook.trim() || null, remote, remote && power);
      setToken("");
      setWebhook("");
    }, () => t("bots.saved"));

  const findChats = () =>
    act(async () => {
      const [bot, chats] = await api.findTelegramChats(token.trim() || null);
      setFound({ bot, chats });
      if (chats.length === 1 && !chat) setChat(chats[0].id);
    });

  return (
    <section className="card">
      <strong>{t("bots.title")}</strong>
      <p className="muted small">{t("bots.intro")}</p>
      {message && <div className={`banner ${message.tone}`}>{message.text}</div>}

      <h3 className="bots-heading">Telegram</h3>
      <ol className="muted small bots-steps">
        <li>{renderInline(t("bots.step1"))}</li>
        <li>{renderInline(t("bots.step2"))}</li>
        <li>{renderInline(t("bots.step3"))}</li>
      </ol>
      <div className="ai-grid">
        <label htmlFor="bot-token">
          {t("bots.token")}
          <input
            id="bot-token"
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={token}
            placeholder={settings.telegram_token ? t("bots.tokenSaved") : "123456789:AA…"}
            onChange={(e) => setToken(e.target.value)}
          />
        </label>
        <label htmlFor="bot-chat">
          {t("bots.chat")}
          <input id="bot-chat" inputMode="numeric" value={chat} placeholder="123456789" onChange={(e) => setChat(e.target.value)} />
        </label>
      </div>
      <div className="actions">
        <button className="secondary" disabled={busy || !hasTelegram} onClick={findChats}>
          {t("bots.findChat")}
        </button>
      </div>
      {found && (
        <div className="small bots-found">
          {found.bot && <p className="muted">{t("bots.botName", { name: found.bot })}</p>}
          {found.chats.length === 0 ? (
            <p className="muted">{t("bots.noChats")}</p>
          ) : (
            found.chats.map((c) => (
              <button key={c.id} className={`chip ${chat === c.id ? "active" : ""}`} onClick={() => setChat(c.id)}>
                {c.name || "?"} · {c.id}
              </button>
            ))
          )}
        </div>
      )}

      <label className="check">
        <input type="checkbox" checked={remote} disabled={busy} onChange={(e) => setRemote(e.target.checked)} />
        <span>
          {t("bots.remote")}
          <span className="muted small block">{t("bots.remoteHelp")}</span>
        </span>
      </label>
      {remote && (
        <label className="check nested">
          <input type="checkbox" checked={power} disabled={busy} onChange={(e) => setPower(e.target.checked)} />
          <span>
            {t("bots.power")}
            <span className="muted small block">{t("bots.powerHelp")}</span>
          </span>
        </label>
      )}

      <h3 className="bots-heading">Discord</h3>
      <p className="muted small">{renderInline(t("bots.discordHelp"))}</p>
      <label htmlFor="bot-webhook">
        {t("bots.webhook")}
        <input
          id="bot-webhook"
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={webhook}
          placeholder={settings.discord_webhook ? t("bots.webhookSaved") : "https://discord.com/api/webhooks/…"}
          onChange={(e) => setWebhook(e.target.value)}
        />
      </label>

      <div className="actions">
        <button className="primary" disabled={busy || !changed} onClick={save}>
          {t("bots.save")}
        </button>
        <button
          className="secondary"
          disabled={busy || changed || (!(settings.telegram_token && settings.telegram_chat) && !settings.discord_webhook)}
          onClick={() => act(api.testBots, (sent) => t("bots.testOk", { where: String(sent) }))}
        >
          {busy ? t("ai.working") : t("bots.test")}
        </button>
        {settings.telegram_token && (
          <button className="link small" disabled={busy} onClick={() => act(() => api.clearBot("telegram"), () => t("bots.removed"))}>
            {t("bots.removeTelegram")}
          </button>
        )}
        {settings.discord_webhook && (
          <button className="link small" disabled={busy} onClick={() => act(() => api.clearBot("discord"), () => t("bots.removed"))}>
            {t("bots.removeDiscord")}
          </button>
        )}
      </div>
      <p className="muted small">{t("bots.privacy")}</p>
    </section>
  );
}
