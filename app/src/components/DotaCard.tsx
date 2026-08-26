import { useEffect, useState } from "react";
import { api, errorMessages, type DotaRole, type DotaSettings, type DotaStatus } from "../api";
import { formatRelative } from "../format";
import { renderInline } from "../guide/GuideText";
import { t, type Key } from "../i18n";
import { openDotaWindow } from "../windowing";

export const DOTA_ROLES: DotaRole[] = ["carry", "mid", "offlane", "soft_support", "hard_support"];

export function roleName(role: DotaRole | null): string {
  return t((role ? `dota.role.${role}` : "dota.role.any") as Key);
}

/** Unix seconds as "5 minutes ago". */
export function ageOf(seconds: number | null | undefined): string {
  return seconds ? formatRelative(new Date(seconds * 1000).toISOString()) : "";
}

/**
 * Settings › Dota 2 companion: the page to open at launch, the player's position,
 * Game State Integration and where the statistics come from.
 */
export default function DotaCard() {
  const [settings, setSettings] = useState<DotaSettings | null>(null);
  const [status, setStatus] = useState<DotaStatus | null>(null);
  const [url, setUrl] = useState("");
  const [role, setRole] = useState<DotaRole | null>(null);
  const [port, setPort] = useState(3417);
  const [assistant, setAssistant] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ tone: "ok" | "error"; text: string } | null>(null);
  /** Set when installing failed: the user creates the file by hand. */
  const [manual, setManual] = useState(false);
  const [key, setKey] = useState("");
  const [keySaved, setKeySaved] = useState(false);

  const load = async () => {
    try {
      const s = await api.dotaGetSettings();
      setSettings(s);
      setUrl(s.launch_url);
      setRole(s.role);
      setPort(s.gsi_port);
      setAssistant(s.launch_assistant);
    } catch {
      // The card stays hidden if the companion can't load (e.g. an older backend).
    }
    api.dotaStatus().then(setStatus).catch(() => {});
    api.dotaKeySaved().then(setKeySaved).catch(() => {});
  };
  useEffect(() => {
    load();
  }, []);

  const act = async (action: () => Promise<unknown>, ok?: string) => {
    setBusy(true);
    setMessage(null);
    try {
      await action();
      if (ok) setMessage({ tone: "ok", text: ok });
      await load();
      return true;
    } catch (e) {
      setMessage({ tone: "error", text: errorMessages(e).join(" ") });
      return false;
    } finally {
      setBusy(false);
    }
  };

  if (!settings) return null;
  const changed =
    url.trim() !== settings.launch_url || role !== settings.role || port !== settings.gsi_port || assistant !== settings.launch_assistant;

  const install = async () => {
    const ok = await act(api.dotaInstallGsi, t("dota.gsi.installedOk"));
    setManual(!ok);
  };

  const copy = () =>
    navigator.clipboard
      .writeText(settings.cfg_text)
      .then(() => setMessage({ tone: "ok", text: t("dota.gsi.copied") }))
      .catch(() => {});

  const source = status?.source;
  const hasData = !!source && source.name !== "none" && !!source.fetched_at;

  return (
    <section className="card">
      <strong>{t("dota.card.title")}</strong>
      <p className="muted small">{t("dota.card.intro")}</p>
      {message && <div className={`banner ${message.tone}`}>{message.text}</div>}

      <label htmlFor="dota-url">
        {t("dota.card.launchUrl")}
        <input
          id="dota-url"
          type="url"
          spellCheck={false}
          value={url}
          placeholder="https://www.dotabuff.com/players/<your id>"
          onChange={(e) => setUrl(e.target.value)}
        />
      </label>
      <p className="muted small">{renderInline(t("dota.card.launchUrlHelp"))}</p>
      <label className="check">
        <input type="checkbox" checked={assistant} disabled={busy} onChange={(e) => setAssistant(e.target.checked)} />
        <span>
          {t("dota.card.assistant")}
          <span className="muted small block">{renderInline(t("dota.card.assistantHelp"))}</span>
        </span>
      </label>

      <div className="ai-grid">
        <label htmlFor="dota-role">
          {t("dota.card.role")}
          <select id="dota-role" value={role ?? ""} onChange={(e) => setRole((e.target.value || null) as DotaRole | null)}>
            <option value="">{roleName(null)}</option>
            {DOTA_ROLES.map((r) => (
              <option key={r} value={r}>
                {roleName(r)}
              </option>
            ))}
          </select>
        </label>
        <label htmlFor="dota-port">
          {t("dota.card.port")}
          <input
            id="dota-port"
            type="number"
            min={1024}
            max={65535}
            value={port}
            onChange={(e) => setPort(Number(e.target.value) || 0)}
          />
        </label>
      </div>
      <div className="actions">
        <button
          className="primary"
          disabled={busy || !changed}
          onClick={() => act(() => api.dotaSetSettings(url.trim(), role, port, assistant), t("dota.card.saved"))}
        >
          {t("dota.card.save")}
        </button>
        <button className="secondary" onClick={() => openDotaWindow(t("dota.window.title")).catch(() => {})}>
          {t("dota.card.openWindow")}
        </button>
      </div>

      <h3 className="bots-heading">{t("dota.gsi.title")}</h3>
      <p className="muted small">{t("dota.gsi.help")}</p>
      {status && (
        <div className="small dota-gsi">
          <span className={`badge ${status.gsi_installed ? "badge-success" : ""}`}>
            {status.gsi_installed ? t("dota.gsi.installed") : t("dota.gsi.notInstalled")}
          </span>{" "}
          {status.gsi_installed && (
            <span className="muted">{status.listening ? t("dota.gsi.listening", { port: status.port }) : t("dota.gsi.notListening")}</span>
          )}
          {status.listen_error && <div className="field-error">{status.listen_error}</div>}
          {status.cfg_path && <div className="muted dota-path">{t("dota.gsi.file", { path: status.cfg_path })}</div>}
          {!status.dota_found && <div className="muted">{t("dota.gsi.notFound")}</div>}
        </div>
      )}
      <div className="actions">
        {!status?.gsi_installed ? (
          <button className="secondary" disabled={busy || changed} onClick={install}>
            {t("dota.gsi.install")}
          </button>
        ) : (
          <button className="link small" disabled={busy} onClick={() => act(api.dotaUninstallGsi, t("dota.gsi.removed"))}>
            {t("dota.gsi.remove")}
          </button>
        )}
      </div>
      <p className="muted small">{renderInline(t("dota.gsi.launchOption"))}</p>
      {manual && (
        <div className="dota-manual">
          <p className="small">{t("dota.gsi.manual")}</p>
          {status?.cfg_path && <p className="muted small dota-path">{status.cfg_path}</p>}
          <pre className="dota-cfg">{settings.cfg_text}</pre>
          <button className="secondary" onClick={copy}>
            {t("dota.gsi.copy")}
          </button>
        </div>
      )}

      <h3 className="bots-heading">{t("dota.data.title")}</h3>
      {source ? (
        <p className="muted small">
          {hasData
            ? [
                t("dota.data.source", { name: source.name }),
                t("dota.data.fresh", { age: ageOf(source.fetched_at) }),
                source.patch ? t("dota.data.patch", { patch: source.patch }) : "",
              ]
                .filter(Boolean)
                .join(" · ")
            : t("dota.data.none")}
          {source.offline && hasData && <span className="block">{t("dota.data.offline")}</span>}
          {source.note && <span className="block">{source.note}</span>}
        </p>
      ) : (
        <p className="muted small">{t("dota.window.loading")}</p>
      )}
      <label htmlFor="dota-key">
        {t("dota.key.label")}
        <input
          id="dota-key"
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={key}
          placeholder={keySaved ? t("dota.key.saved") : t("dota.key.placeholder")}
          onChange={(e) => setKey(e.target.value)}
        />
      </label>
      <div className="actions">
        <button className="secondary" disabled={busy || !key.trim()} onClick={() => act(async () => { await api.dotaSetKey(key.trim()); setKey(""); }, t("dota.key.done"))}>
          {t("dota.key.save")}
        </button>
        {keySaved && (
          <button className="link small" disabled={busy} onClick={() => act(() => api.dotaSetKey(null), t("dota.key.removed"))}>
            {t("dota.key.remove")}
          </button>
        )}
      </div>
      <p className="muted small">{t("dota.key.help")}</p>
      <p className="muted small">{t("dota.card.privacy")}</p>
    </section>
  );
}
