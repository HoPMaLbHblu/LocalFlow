import { useEffect, useMemo, useState } from "react";
import { api, errorMessages, type Link, type LinkSet } from "../api";
import { t } from "../i18n";

const BROWSERS = ["default", "chrome", "edge", "firefox", "brave", "opera", "yandex"];

interface Props {
  /** Opens the editor with a new automation that opens this set. */
  onAutomate: (title: string, code: string) => void;
}

function blank(): LinkSet {
  return { name: "", links: [], browser: "default", new_window: true, updated_at: 0 };
}

/** Lua string literal for a set name. */
function luaString(text: string): string {
  return JSON.stringify(text);
}

/** Links page: named sets of web addresses that open together in a browser. */
export default function LinksView({ onAutomate }: Props) {
  const [sets, setSets] = useState<LinkSet[]>([]);
  const [trash, setTrash] = useState<LinkSet[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState<LinkSet>(blank());
  const [paste, setPaste] = useState("");
  const [folder, setFolder] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ tone: "ok" | "error"; text: string } | null>(null);

  const load = async (select?: string | null) => {
    const list = await api.linksList();
    setSets(list);
    setTrash(await api.linksTrash());
    const name = select === undefined ? selected : select;
    const found = list.find((s) => s.name === name);
    if (found) {
      setSelected(found.name);
      setDraft(structuredClone(found));
    } else if (select === null || !name) {
      setSelected(null);
      setDraft(blank());
    }
  };
  useEffect(() => {
    load().catch(() => {});
  }, []);

  const saved = sets.find((s) => s.name === selected) ?? null;
  const dirty = useMemo(() => JSON.stringify(saved ?? blank()) !== JSON.stringify(draft), [saved, draft]);

  const act = async (action: () => Promise<unknown>, ok?: (result: unknown) => string) => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await action();
      if (ok) setMessage({ tone: "ok", text: ok(result) });
    } catch (e) {
      setMessage({ tone: "error", text: errorMessages(e).join(" ") });
    } finally {
      setBusy(false);
    }
  };

  const save = () =>
    act(async () => {
      const result = await api.linksSave(draft, selected);
      await load(result.name);
    }, () => t("links.saved"));

  const addPasted = () =>
    act(async () => {
      const found: Link[] = await api.linksParse(paste);
      if (found.length === 0) throw new Error(t("links.noneFound"));
      const known = new Set(draft.links.map((l) => l.url));
      const fresh = found.filter((l) => !known.has(l.url));
      setDraft({ ...draft, links: [...draft.links, ...fresh] });
      setPaste("");
      return fresh.length;
    }, (n) => t("links.added", { n: String(n) }));

  const importFolder = () =>
    act(async () => {
      const found = await api.linksImportBookmarks(folder, draft.browser === "firefox" ? "default" : draft.browser);
      const known = new Set(draft.links.map((l) => l.url));
      const fresh = found.filter((l) => !known.has(l.url));
      setDraft({ ...draft, links: [...draft.links, ...fresh], name: draft.name || folder });
      return fresh.length;
    }, (n) => t("links.added", { n: String(n) }));

  const updateLink = (i: number, change: Partial<Link>) =>
    setDraft({ ...draft, links: draft.links.map((l, j) => (j === i ? { ...l, ...change } : l)) });
  const move = (i: number, by: number) => {
    const links = [...draft.links];
    const j = i + by;
    if (j < 0 || j >= links.length) return;
    [links[i], links[j]] = [links[j], links[i]];
    setDraft({ ...draft, links });
  };

  return (
    <div className="page links-page">
      <h1>{t("links.title")}</h1>
      <p className="muted">{t("links.intro")}</p>
      {message && <div className={`banner ${message.tone}`}>{message.text}</div>}

      <div className="links-layout">
        <aside className="card links-list">
          <button className={`links-item ${selected === null ? "selected" : ""}`} onClick={() => { setSelected(null); setDraft(blank()); }}>
            {t("links.new")}
          </button>
          {sets.map((s) => (
            <button key={s.name} className={`links-item ${selected === s.name ? "selected" : ""}`} onClick={() => { setSelected(s.name); setDraft(structuredClone(s)); }}>
              <span>{s.name}</span>
              <span className="muted small">{t("links.count", { n: String(s.links.length) })}</span>
            </button>
          ))}
          {trash.length > 0 && (
            <details className="links-trash">
              <summary className="muted small">{t("links.trash", { n: String(trash.length) })}</summary>
              {trash.map((s, i) => (
                <div key={`${s.name}-${i}`} className="links-trash-row small">
                  <span>{s.name}</span>
                  <button className="link small" disabled={busy} onClick={() => act(async () => { await api.linksRestore(s.name); await load(); }, () => t("links.restored"))}>
                    {t("links.restore")}
                  </button>
                </div>
              ))}
            </details>
          )}
        </aside>

        <section className="card links-editor">
          <div className="fields">
            <label className="grow">
              {t("links.name")}
              <input value={draft.name} placeholder={t("links.namePlaceholder")} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
            </label>
            <label>
              {t("links.browser")}
              <select value={draft.browser} onChange={(e) => setDraft({ ...draft, browser: e.target.value })}>
                {BROWSERS.map((b) => (
                  <option key={b} value={b}>{t(`links.browser.${b}` as never)}</option>
                ))}
              </select>
            </label>
          </div>
          <label className="check">
            <input type="checkbox" checked={draft.new_window} onChange={(e) => setDraft({ ...draft, new_window: e.target.checked })} />
            <span>{t("links.newWindow")}</span>
          </label>

          <table className="links-table">
            <thead>
              <tr><th>#</th><th>{t("links.titleColumn")}</th><th>{t("links.url")}</th><th /></tr>
            </thead>
            <tbody>
              {draft.links.map((l, i) => (
                <tr key={i}>
                  <td className="muted small">{i + 1}</td>
                  <td><input value={l.title} placeholder={t("links.optional")} onChange={(e) => updateLink(i, { title: e.target.value })} /></td>
                  <td><input value={l.url} spellCheck={false} onChange={(e) => updateLink(i, { url: e.target.value })} /></td>
                  <td className="links-row-actions">
                    <button className="link small" title={t("links.up")} onClick={() => move(i, -1)}>↑</button>
                    <button className="link small" title={t("links.down")} onClick={() => move(i, 1)}>↓</button>
                    <button className="link small" title={t("links.removeRow")} onClick={() => setDraft({ ...draft, links: draft.links.filter((_, j) => j !== i) })}>✕</button>
                  </td>
                </tr>
              ))}
              {draft.links.length === 0 && (
                <tr><td colSpan={4} className="muted small">{t("links.empty")}</td></tr>
              )}
            </tbody>
          </table>
          <button className="link small" onClick={() => setDraft({ ...draft, links: [...draft.links, { url: "", title: "" }] })}>
            {t("links.addRow")}
          </button>

          <label>
            {t("links.paste")}
            <textarea rows={3} value={paste} spellCheck={false} placeholder={"https://example.com\nMail | https://mail.example.com"} onChange={(e) => setPaste(e.target.value)} />
          </label>
          <div className="actions">
            <button className="secondary" disabled={busy || !paste.trim()} onClick={addPasted}>{t("links.addPasted")}</button>
          </div>

          <label>
            {t("links.bookmarks")}
            <input value={folder} placeholder={t("links.bookmarksPlaceholder")} onChange={(e) => setFolder(e.target.value)} />
          </label>
          <p className="muted small">{t("links.bookmarksHelp")}</p>
          <div className="actions">
            <button className="secondary" disabled={busy || !folder.trim()} onClick={importFolder}>{t("links.import")}</button>
          </div>

          <div className="actions links-main-actions">
            <button className="primary" disabled={busy || !dirty || !draft.name.trim()} onClick={save}>{t("links.save")}</button>
            <button
              className="secondary"
              disabled={busy || !saved || dirty}
              title={dirty ? t("links.saveFirst") : undefined}
              onClick={() => act(() => api.linksOpen(draft.name), (n) => t("links.opened", { n: String(n) }))}
            >
              {t("links.open", { n: String(draft.links.length) })}
            </button>
            <button
              className="secondary"
              disabled={!saved || dirty}
              onClick={() => onAutomate(t("links.automationTitle", { name: draft.name }), `-- Opens the link set ${luaString(draft.name)} (edit it on the Links page).\n-- Pick a schedule or hotkey above.\n\nautomation {\n    name = ${luaString(t("links.automationTitle", { name: draft.name }))},\n\n    run = function(ctx)\n        links.open(${luaString(draft.name)})\n    end\n}\n`)}
            >
              {t("links.automate")}
            </button>
            {saved && (
              <button className="link small push-right" disabled={busy} onClick={() => act(async () => { await api.linksDelete(saved.name); await load(null); }, () => t("links.deleted"))}>
                {t("links.delete")}
              </button>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}
