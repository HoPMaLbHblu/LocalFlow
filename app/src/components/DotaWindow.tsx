import { useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  errorMessages,
  onCoreEvent,
  type DotaDraft,
  type DotaEvidence,
  type DotaHero,
  type DotaHeroLookup,
  type DotaHeroSuggestion,
  type DotaItemAdvice,
  type DotaItemPlan,
  type DotaLive,
  type DotaMatchReview,
  type DotaMatchSummary,
  type DotaMatchup,
  type DotaReason,
  type DotaRole,
  type DotaSlot,
  type DotaStatus,
  type DotaTeam,
} from "../api";
import { t, type Key } from "../i18n";
import { DOTA_ROLES, ageOf, roleName } from "./DotaCard";

const HERO_LIST_ID = "dota-hero-list";

type Tab = "draft" | "items" | "lookup" | "review";
const TABS: Tab[] = ["draft", "items", "lookup", "review"];
const TAB_KEY = "localflow.dota.tab";

/** A text field with the hero names as suggestions. Commits on Enter or when leaving it. */
function HeroInput({ value, onPick, disabled, label }: { value: string | null; onPick: (hero: string | null) => void; disabled?: boolean; label: string }) {
  const [text, setText] = useState(value ?? "");
  useEffect(() => setText(value ?? ""), [value]);
  const commit = () => {
    const wanted = text.trim();
    if (wanted === (value ?? "")) return;
    onPick(wanted || null);
  };
  return (
    <input
      className="dota-hero-input"
      list={HERO_LIST_ID}
      aria-label={label}
      value={text}
      disabled={disabled}
      placeholder={t("dota.window.slotPlaceholder")}
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
        if (e.key === "Escape") setText(value ?? "");
      }}
    />
  );
}

/** "Data" (statistics, with source and age) or "Rule of thumb". */
function EvidenceTag({ evidence }: { evidence: DotaEvidence }) {
  if (evidence.kind === "sourced") {
    const detail = [evidence.source, evidence.detail, ageOf(evidence.fetched_at)].filter(Boolean).join(" · ");
    return (
      <>
        <span className="dota-tag dota-tag-data" title={detail}>
          {t("dota.tag.data")}
        </span>{" "}
        <span className="muted small">{detail}</span>
      </>
    );
  }
  return (
    <>
      <span className="dota-tag dota-tag-rule" title={evidence.rule}>
        {t("dota.tag.rule")}
      </span>{" "}
      <span className="muted small">{evidence.rule}</span>
    </>
  );
}

function ReasonLine({ reason }: { reason: DotaReason }) {
  return (
    <li>
      {reason.text} <EvidenceTag evidence={reason.evidence} />
    </li>
  );
}

function ItemList({ title, items }: { title: string; items: DotaItemAdvice[] }) {
  if (items.length === 0) return null;
  return (
    <>
      <h4 className="dota-subheading">{title}</h4>
      <ol className="dota-items">
        {[...items]
          .sort((a, b) => a.priority - b.priority)
          .map((item) => (
            <li key={item.key + item.priority}>
              <strong>{item.item}</strong> <span className="muted small">#{item.priority}</span>
              <div className="small">
                {item.why} <EvidenceTag evidence={item.evidence} />
              </div>
              {item.alternatives.length > 0 && (
                <div className="muted small">
                  {t("dota.window.alternatives")} {item.alternatives.join(", ")}
                </div>
              )}
            </li>
          ))}
      </ol>
    </>
  );
}

/** Game-clock seconds as "12:34" ("-0:45" before the horn). */
function clockText(seconds: number): string {
  const sign = seconds < 0 ? "-" : "";
  const abs = Math.abs(Math.round(seconds));
  const h = Math.floor(abs / 3600);
  const m = Math.floor((abs % 3600) / 60);
  const s = String(abs % 60).padStart(2, "0");
  return h > 0 ? `${sign}${h}:${String(m).padStart(2, "0")}:${s}` : `${sign}${m}:${s}`;
}

/** The kind of trouble behind a review error, for a helpful message. */
function reviewTrouble(message: string): "account" | "private" | "offline" | "other" {
  if (/account isn't set|account id/i.test(message)) return "account";
  if (/private|public match data|expose/i.test(message)) return "private";
  if (/network|internet|offline|connect|timed out|rate limit|HTTP 5/i.test(message)) return "offline";
  return "other";
}

/** The Live panel: shown in the Items tab only while Game State Integration sends match data. */
function LivePanel({ live }: { live: DotaLive }) {
  const next = live.next_item;
  const stale = Date.now() / 1000 - live.state.updated_at > 60;
  return (
    <section className="dota-section dota-live">
      <div className="dota-row">
        <h3 className="dota-heading">{t("dota.live.title")}</h3>
        <span className="dota-live-dot" aria-hidden="true" />
        <span className="muted small">{t("dota.live.clock", { time: clockText(live.state.clock) })}</span>
        <strong className="dota-gold push-right">{t("dota.live.gold", { gold: live.state.gold })}</strong>
      </div>
      {!live.state.alive && <p className="muted small">{t("dota.live.dead")}</p>}
      {stale && <p className="muted small">{t("dota.live.stale")}</p>}
      <h4 className="dota-subheading">{t("dota.live.next")}</h4>
      {next ? (
        <div className="dota-next">
          <strong>{next.advice.item}</strong>{" "}
          {next.affordable ? (
            <span className="badge badge-success">{t("dota.live.affordable")}</span>
          ) : (
            <span className="muted small">{t("dota.live.missing", { gold: next.missing_gold })}</span>
          )}
          {!next.affordable && next.missing_gold > 0 && (
            <div className="dota-bar" role="progressbar" aria-valuemin={0} aria-valuemax={100}
              aria-valuenow={Math.round((live.state.gold / (live.state.gold + next.missing_gold)) * 100)}>
              <span style={{ width: `${Math.min(100, (live.state.gold / (live.state.gold + next.missing_gold)) * 100)}%` }} />
            </div>
          )}
          <div className="small">
            {next.advice.why} <EvidenceTag evidence={next.advice.evidence} />
          </div>
        </div>
      ) : (
        <p className="muted small">{live.next_note ?? t("dota.live.noNext")}</p>
      )}
      <h4 className="dota-subheading">{t("dota.live.reminders")}</h4>
      {live.reminders.length === 0 ? (
        <p className="muted small">{t("dota.live.noReminders")}</p>
      ) : (
        <ul className="dota-reminders small">
          {[...live.reminders]
            .sort((a, b) => a.clock - b.clock)
            .map((r, i) => (
              <li key={i}>
                <span className="dota-reminder-time">{clockText(r.clock)}</span> {r.text}
              </li>
            ))}
        </ul>
      )}
    </section>
  );
}

function MatchupList({ title, list }: { title: string; list: DotaMatchup[] }) {
  return (
    <>
      <h4 className="dota-subheading">{title}</h4>
      {list.length === 0 ? (
        <p className="muted small">{t("dota.lookup.none")}</p>
      ) : (
        <ol className="dota-items">
          {list.map((m) => (
            <li key={m.hero_id}>
              <strong>{m.hero}</strong>
              <div className="small">
                {m.reason.text} <EvidenceTag evidence={m.reason.evidence} />
              </div>
            </li>
          ))}
        </ol>
      )}
    </>
  );
}

/**
 * Common items. When the source labels them by stage in `why` ("Starting items", "Early game",
 * ...), they are grouped under those labels; otherwise shown as one list.
 */
function CommonItems({ items }: { items: DotaItemAdvice[] }) {
  if (items.length === 0) return <p className="muted small">{t("dota.lookup.none")}</p>;
  // `why` reads "Starting items: bought 178 times ..."; the stage is the part before the colon.
  const stageOf = (i: DotaItemAdvice) => i.why.split(":")[0];
  const stages = [...new Set(items.map(stageOf))];
  if (stages.length > 1 && stages.length <= 5 && stages.length < items.length) {
    return (
      <>
        {stages.map((stage) => (
          <div key={stage}>
            <div className="small dota-stage">{stage}</div>
            <div className="dota-chips">
              {items
                .filter((i) => stageOf(i) === stage)
                .sort((a, b) => a.priority - b.priority)
                .map((i) => (
                  <span key={i.key} className="dota-item-chip" title={i.evidence.kind === "sourced" ? i.evidence.detail : i.evidence.rule}>
                    {i.item}
                  </span>
                ))}
            </div>
          </div>
        ))}
      </>
    );
  }
  return <ItemList title="" items={items} />;
}

function LookupTab({ heroes, heroError }: { heroes: DotaHero[]; heroError: string | null }) {
  const [query, setQuery] = useState("");
  const [picked, setPicked] = useState<DotaHero | null>(null);
  const [result, setResult] = useState<DotaHeroLookup | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const matches = useMemo(() => {
    const q = query.trim().toLowerCase().replace(/[^a-z0-9а-яё]/g, "");
    const list = q
      ? heroes.filter((h) => [h.localized_name, h.name.replace("npc_dota_hero_", "")].some((n) => n.toLowerCase().replace(/[^a-z0-9а-яё]/g, "").includes(q)))
      : heroes;
    return list.slice(0, q ? 40 : 200);
  }, [heroes, query]);

  const pick = async (hero: DotaHero) => {
    setPicked(hero);
    setQuery("");
    setLoading(true);
    setError(null);
    setResult(null);
    try {
      setResult(await api.dotaLookup(String(hero.id), 6));
    } catch (e) {
      setError(errorMessages(e).join(" "));
    } finally {
      setLoading(false);
    }
  };

  return (
    <>
      <section className="dota-section">
        <label className="dota-lookup-search small">
          {t("dota.lookup.search")}
          <input
            type="search"
            value={query}
            placeholder={t("dota.lookup.placeholder")}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && matches.length > 0) pick(matches[0]);
            }}
          />
        </label>
        {heroError && <p className="muted small">{t("dota.window.heroesMissing", { error: heroError })}</p>}
        {(query.trim() !== "" || !picked) && heroes.length > 0 && (
          <div className="dota-chips dota-hero-picker">
            {matches.length === 0 ? (
              <span className="muted small">{t("dota.lookup.noMatch", { q: query.trim() })}</span>
            ) : (
              matches.map((h) => (
                <button key={h.id} className={`chip ${picked?.id === h.id ? "active" : ""}`} onClick={() => pick(h)}>
                  {h.localized_name}
                </button>
              ))
            )}
          </div>
        )}
        {!picked && <p className="muted small">{t("dota.lookup.pick")}</p>}
      </section>

      {picked && (
        <section className="dota-section">
          <div className="dota-row">
            <h3 className="dota-heading">{picked.localized_name}</h3>
            <button className="link small push-right" onClick={() => setPicked(null)}>
              {t("dota.lookup.another")}
            </button>
          </div>
          {loading && <p className="muted small">{t("dota.lookup.loading")}</p>}
          {error && <p className="muted small">{error}</p>}
          {result && (
            <>
              {result.traits.length > 0 && (
                <div className="dota-chips">
                  {result.traits.map((tr) => (
                    <span key={tr} className="dota-tag dota-tag-rule">
                      {tr}
                    </span>
                  ))}
                </div>
              )}
              <MatchupList title={t("dota.lookup.strong")} list={result.strong_against} />
              <MatchupList title={t("dota.lookup.weak")} list={result.weak_against} />
              <h4 className="dota-subheading">{t("dota.lookup.items")}</h4>
              <CommonItems items={result.common_items} />
              {result.data_note && <p className="muted small">{result.data_note}</p>}
            </>
          )}
        </section>
      )}
    </>
  );
}

/** Paste a Dotabuff / OpenDota / STRATZ link or a Steam id; saved on this PC. */
function AccountForm({ onSaved, current }: { onSaved: (id: number | null) => void; current: number | null }) {
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      onSaved(await api.dotaSetAccount(text.trim()));
    } catch (e) {
      setError(errorMessages(e).join(" "));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="dota-section">
      <h3 className="dota-heading">{t("dota.review.accountTitle")}</h3>
      <p className="muted small">{t("dota.review.accountHelp")}</p>
      <div className="dota-row">
        <input
          className="dota-account-input"
          spellCheck={false}
          value={text}
          aria-label={t("dota.review.accountTitle")}
          placeholder="https://www.opendota.com/players/123456789"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && text.trim() && save()}
        />
        <button className="primary" disabled={busy || !text.trim()} onClick={save}>
          {t("dota.review.accountSave")}
        </button>
        {current != null && (
          <button className="link small" disabled={busy} onClick={() => onSaved(current)}>
            {t("dota.review.cancel")}
          </button>
        )}
      </div>
      {error && <div className="field-error small">{error}</div>}
    </section>
  );
}

function durationText(secs: number): string {
  return clockText(secs);
}

function MatchRow({ m }: { m: DotaMatchSummary }) {
  return (
    <li className="dota-match-row small">
      <span className={`dota-result ${m.won ? "won" : "lost"}`}>{m.won ? t("dota.review.wonShort") : t("dota.review.lostShort")}</span>
      <strong>{m.hero}</strong>
      <span>
        {m.kills}/{m.deaths}/{m.assists}
      </span>
      <span className="muted">{t("dota.review.gpmValue", { n: m.gpm })}</span>
      <span className="muted">{durationText(m.duration_secs)}</span>
      <span className="muted push-right">{ageOf(m.start_time)}</span>
    </li>
  );
}

function ReviewTab() {
  const [account, setAccount] = useState<number | null | undefined>(undefined);
  const [editing, setEditing] = useState(false);
  const [review, setReview] = useState<DotaMatchReview | null>(null);
  const [reviewError, setReviewError] = useState<string | null>(null);
  const [recent, setRecent] = useState<DotaMatchSummary[] | null>(null);
  const [recentError, setRecentError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const load = () => {
    setLoading(true);
    setReviewError(null);
    setRecentError(null);
    Promise.allSettled([
      api.dotaLastMatch().then(setReview, (e) => {
        setReview(null);
        setReviewError(errorMessages(e).join(" "));
      }),
      api.dotaRecentMatches(10).then(setRecent, (e) => {
        setRecent(null);
        setRecentError(errorMessages(e).join(" "));
      }),
    ]).finally(() => setLoading(false));
  };

  useEffect(() => {
    api
      .dotaGetSettings()
      .then((s) => setAccount(s.account_id))
      .catch(() => setAccount(null));
  }, []);
  useEffect(() => {
    if (account != null && !editing) load();
  }, [account, editing]);

  if (account === undefined) return <p className="muted small">{t("dota.window.loading")}</p>;
  if (account === null || editing) {
    return (
      <AccountForm
        current={account}
        onSaved={(id) => {
          setEditing(false);
          setAccount(id);
        }}
      />
    );
  }

  const trouble = reviewError ? reviewTrouble(reviewError) : null;
  const s = review?.summary;
  return (
    <>
      <section className="dota-section">
        <div className="dota-row">
          <h3 className="dota-heading">{t("dota.review.title")}</h3>
          <span className="muted small">{t("dota.review.account", { id: account })}</span>
          <button className="link small push-right" onClick={() => setEditing(true)}>
            {t("dota.review.accountChange")}
          </button>
          <button className="link small" disabled={loading} onClick={load}>
            {t("dota.window.refresh")}
          </button>
        </div>
        {loading && !review && <p className="muted small">{t("dota.review.loading")}</p>}
        {reviewError && (
          <div className="dota-empty small">
            {trouble === "private" && <p>{t("dota.review.private")}</p>}
            {trouble === "offline" && <p>{t("dota.review.offline")}</p>}
            {trouble === "account" && <p>{t("dota.review.noAccount")}</p>}
            <p className="muted">{reviewError}</p>
          </div>
        )}
        {s && review && (
          <>
            <div className="dota-match-card">
              <div className="dota-row">
                <strong className="dota-match-hero">{s.hero}</strong>
                <span className={`dota-result ${s.won ? "won" : "lost"}`}>{s.won ? t("dota.review.won") : t("dota.review.lost")}</span>
                <span className="muted small push-right">{ageOf(s.start_time)}</span>
              </div>
              <div className="dota-stats">
                <div>
                  <span className="muted small">{t("dota.review.kda")}</span>
                  <strong>
                    {s.kills}/{s.deaths}/{s.assists}
                  </strong>
                </div>
                <div>
                  <span className="muted small">{t("dota.review.gpm")}</span>
                  <strong>{s.gpm}</strong>
                </div>
                <div>
                  <span className="muted small">{t("dota.review.xpm")}</span>
                  <strong>{s.xpm}</strong>
                </div>
                <div>
                  <span className="muted small">{t("dota.review.lastHits")}</span>
                  <strong>{s.last_hits}</strong>
                </div>
                <div>
                  <span className="muted small">{t("dota.review.duration")}</span>
                  <strong>{durationText(s.duration_secs)}</strong>
                </div>
              </div>
              {s.items.length > 0 && <div className="muted small">{s.items.join(" · ")}</div>}
            </div>

            {review.benchmarks.length > 0 && (
              <>
                <h4 className="dota-subheading">{t("dota.review.benchmarks", { hero: s.hero })}</h4>
                <ul className="dota-benchmarks small">
                  {review.benchmarks.map((b) => (
                    <li key={b.metric}>
                      <span className="dota-bench-label">
                        {b.metric}: <strong>{Number.isInteger(b.value) ? b.value : b.value.toFixed(1)}</strong>
                      </span>
                      {b.percentile != null ? (
                        <>
                          <span className="dota-bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(b.percentile * 100)}>
                            <span className={b.percentile < 0.34 ? "low" : b.percentile < 0.67 ? "mid" : "high"} style={{ width: `${Math.round(b.percentile * 100)}%` }} />
                          </span>
                          <span className="muted">{t("dota.review.percentile", { p: Math.round(b.percentile * 100) })}</span>
                        </>
                      ) : (
                        <span className="muted">{t("dota.review.noPercentile")}</span>
                      )}
                    </li>
                  ))}
                </ul>
              </>
            )}
            {review.notes.length > 0 && (
              <>
                <h4 className="dota-subheading">{t("dota.review.notes")}</h4>
                <ul className="dota-reasons small">
                  {review.notes.map((r, i) => (
                    <ReasonLine key={i} reason={r} />
                  ))}
                </ul>
              </>
            )}
            {review.data_note && <p className="muted small">{review.data_note}</p>}
          </>
        )}
      </section>

      <section className="dota-section">
        <h3 className="dota-heading">{t("dota.review.recent")}</h3>
        {recentError && !reviewError && <p className="muted small">{recentError}</p>}
        {recent && recent.length === 0 && <p className="muted small">{t("dota.review.noRecent")}</p>}
        {recent && recent.length > 0 && (
          <ul className="dota-matches">
            {recent.map((m) => (
              <MatchRow key={m.match_id} m={m} />
            ))}
          </ul>
        )}
      </section>
      <p className="muted small">{t("dota.review.privacy")}</p>
    </>
  );
}

function savedTab(): Tab {
  try {
    const tab = localStorage.getItem(TAB_KEY) as Tab | null;
    if (tab && TABS.includes(tab)) return tab;
  } catch {
    // Storage may be unavailable; start on the draft.
  }
  return "draft";
}

/** The Dota 2 companion in its own window: the draft, items (with the live helper), hero lookup, review. */
export default function DotaWindow() {
  const [tab, setTabState] = useState<Tab>(savedTab);
  const [status, setStatus] = useState<DotaStatus | null>(null);
  const [draft, setDraft] = useState<DotaDraft | null>(null);
  const [heroes, setHeroes] = useState<DotaHero[]>([]);
  const [heroError, setHeroError] = useState<string | null>(null);
  const [suggestions, setSuggestions] = useState<DotaHeroSuggestion[] | null>(null);
  const [suggestError, setSuggestError] = useState<string | null>(null);
  const [plan, setPlan] = useState<DotaItemPlan | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [live, setLive] = useState<DotaLive | null>(null);
  const [busy, setBusy] = useState(false);
  const [capturing, setCapturing] = useState(false);
  const [message, setMessage] = useState<{ tone: "ok" | "error"; lines: string[] } | null>(null);
  const timer = useRef<number | undefined>(undefined);

  const setTab = (next: Tab) => {
    setTabState(next);
    try {
      localStorage.setItem(TAB_KEY, next);
    } catch {
      // Only a convenience.
    }
  };

  const loadHeroes = () =>
    api
      .dotaHeroes()
      .then((list) => {
        setHeroes(list);
        setHeroError(null);
      })
      .catch((e) => setHeroError(errorMessages(e).join(" ")));

  const loadLive = () =>
    api
      .dotaLive()
      .then(setLive)
      .catch(() => setLive(null));

  const loadAdvice = (d: DotaDraft) => {
    api
      .dotaSuggest(8)
      .then((list) => {
        setSuggestions(list);
        setSuggestError(null);
      })
      .catch((e) => {
        setSuggestions(null);
        setSuggestError(errorMessages(e).join(" "));
      });
    if (d.player_hero_id == null) {
      setPlan(null);
      setPlanError(null);
      return;
    }
    api
      .dotaBuild(null)
      .then((p) => {
        setPlan(p);
        setPlanError(null);
      })
      .catch((e) => {
        setPlan(null);
        setPlanError(errorMessages(e).join(" "));
      });
  };

  const refresh = async () => {
    api.dotaStatus().then(setStatus).catch(() => {});
    loadLive();
    try {
      const d = await api.dotaDraft();
      setDraft(d);
      loadAdvice(d);
    } catch (e) {
      setMessage({ tone: "error", lines: errorMessages(e) });
    }
  };

  useEffect(() => {
    document.title = t("dota.window.title");
    loadHeroes();
    refresh();
    // Game State Integration and scripts change the draft: refresh shortly after (once per burst).
    const unlisten = onCoreEvent((event) => {
      if (event.type !== "dota_changed") return;
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(refresh, 400);
    });
    const poll = window.setInterval(() => api.dotaStatus().then(setStatus).catch(() => {}), 5000);
    return () => {
      unlisten.then((f) => f());
      window.clearInterval(poll);
      window.clearTimeout(timer.current);
    };
  }, []);

  // Gold and the clock change every second: poll the live helper while the Items tab is open.
  useEffect(() => {
    if (tab !== "items") return;
    loadLive();
    const poll = window.setInterval(loadLive, 3000);
    return () => window.clearInterval(poll);
  }, [tab]);

  const act = async (action: () => Promise<DotaDraft | void>, ok?: (d: DotaDraft | void) => string[]) => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await action();
      if (result) {
        setDraft(result);
        loadAdvice(result);
      }
      if (ok) setMessage({ tone: "ok", lines: ok(result) });
    } catch (e) {
      setMessage({ tone: "error", lines: errorMessages(e) });
    } finally {
      setBusy(false);
    }
  };

  const capture = async () => {
    setCapturing(true);
    setMessage(null);
    try {
      const report = await api.dotaCapture();
      setDraft(report.draft);
      loadAdvice(report.draft);
      setMessage({ tone: report.warnings.length ? "error" : "ok", lines: [t("dota.window.recognized", { n: report.recognized }), ...report.warnings] });
      if (heroes.length === 0) loadHeroes();
    } catch (e) {
      setMessage({ tone: "error", lines: errorMessages(e) });
    } finally {
      setCapturing(false);
    }
  };

  const slotRow = (side: "allies" | "enemies", slot: DotaSlot) => (
    <li key={`${side}-${slot.slot}`} className={`dota-slot ${slot.uncertain ? "dota-slot-uncertain" : ""}`}>
      <span className="dota-slot-n">{slot.slot}</span>
      <HeroInput
        value={slot.hero}
        label={`${side === "allies" ? t("dota.window.allies") : t("dota.window.enemies")} ${slot.slot}`}
        disabled={busy || capturing}
        onPick={(hero) => act(() => api.dotaCorrect(side, slot.slot, hero))}
      />
      <span className="dota-slot-meta small">
        {slot.hero_id == null
          ? ""
          : slot.source === "manual"
            ? t("dota.window.byYou")
            : slot.source === "gsi"
              ? t("dota.window.byGame")
              : `${Math.round(slot.confidence * 100)}%`}
        {slot.uncertain && (
          <span className="badge badge-running dota-check" title={t("dota.window.checkHelp")}>
            {t("dota.window.check")}
          </span>
        )}
      </span>
      {slot.uncertain && slot.alternatives.length > 0 && (
        <span className="dota-alternatives small">
          {t("dota.window.alternatives")}{" "}
          {slot.alternatives.slice(0, 3).map((a) => (
            <button key={a.hero_id} className="chip" disabled={busy} onClick={() => act(() => api.dotaCorrect(side, slot.slot, String(a.hero_id)))}>
              {a.hero} {Math.round(a.confidence * 100)}%
            </button>
          ))}
        </span>
      )}
    </li>
  );

  const phaseLine = () => {
    if (!status) return t("dota.window.loading");
    if (!status.dota_running) return t("dota.window.notRunning");
    return t(`dota.phase.${status.state}` as Key);
  };

  const setTeam = (team: DotaTeam) => act(() => api.dotaSetTeam(team));

  return (
    <div className="dota-window">
      <datalist id={HERO_LIST_ID}>
        {heroes.map((h) => (
          <option key={h.id} value={h.localized_name} />
        ))}
      </datalist>

      <header className="dota-header">
        <strong>{t("dota.window.title")}</strong>
        <span className="muted small">{phaseLine()}</span>
      </header>
      <nav className="tabs dota-tabs" role="tablist">
        {TABS.map((id) => (
          <button key={id} role="tab" aria-selected={tab === id} className={tab === id ? "active" : ""} onClick={() => setTab(id)}>
            {t(`dota.tab.${id}` as Key)}
            {id === "items" && live && <span className="dota-live-dot" aria-label={t("dota.live.title")} />}
          </button>
        ))}
      </nav>
      {status && !status.gsi_installed && (tab === "draft" || tab === "items") && <p className="muted small">{t("dota.window.noGsi")}</p>}
      {heroError && tab !== "lookup" && <p className="muted small">{t("dota.window.heroesMissing", { error: heroError })}</p>}
      {message && (
        <div className={`banner dota-banner ${message.tone}`}>
          {message.lines.map((line, i) => (
            <div key={i}>{line}</div>
          ))}
        </div>
      )}

      {tab === "draft" && draft && (
        <section className="dota-section">
          <div className="dota-row">
            <span className="small">{t("dota.window.team")}</span>
            <div className="segmented" role="group" aria-label={t("dota.window.team")}>
              {(["radiant", "dire"] as DotaTeam[]).map((team) => (
                <button
                  key={team}
                  className={draft.team === team && !draft.team_assumed ? "active" : draft.team === team ? "active assumed" : ""}
                  disabled={busy}
                  onClick={() => setTeam(team)}
                >
                  {t(`dota.team.${team}` as Key)}
                </button>
              ))}
            </div>
            <label className="dota-inline small">
              {t("dota.window.role")}
              <select
                value={draft.role ?? ""}
                disabled={busy}
                onChange={(e) => act(() => api.dotaSetRole((e.target.value || null) as DotaRole | null))}
              >
                <option value="">{roleName(null)}</option>
                {DOTA_ROLES.map((r) => (
                  <option key={r} value={r}>
                    {roleName(r)}
                  </option>
                ))}
              </select>
            </label>
          </div>
          {draft.team_assumed && <p className="muted small">{t("dota.window.teamAssumed")}</p>}
          <div className="dota-row">
            <span className="small">{t("dota.window.yourHero")}</span>
            <HeroInput value={draft.player_hero} label={t("dota.window.yourHero")} disabled={busy} onPick={(hero) => act(() => api.dotaSetHero(hero))} />
          </div>

          <div className="actions">
            <button className="primary" disabled={busy || capturing} onClick={capture}>
              {capturing ? t("dota.window.capturing") : t("dota.window.capture")}
            </button>
            <button className="secondary" disabled={busy || capturing} onClick={() => act(api.dotaReset)}>
              {t("dota.window.reset")}
            </button>
          </div>
          <p className="muted small">{t("dota.window.captureHelp")}</p>
          {draft.note && <p className="muted small">{draft.note}</p>}

          <h3 className="dota-heading">{t("dota.window.allies")}</h3>
          <ol className="dota-slots">{draft.allies.map((s) => slotRow("allies", s))}</ol>
          <h3 className="dota-heading">{t("dota.window.enemies")}</h3>
          <ol className="dota-slots">{draft.enemies.map((s) => slotRow("enemies", s))}</ol>
        </section>
      )}

      {tab === "draft" && (
        <section className="dota-section">
          <div className="dota-row">
            <h3 className="dota-heading">{t("dota.window.suggestions")}</h3>
            <button className="link small push-right" disabled={busy || !draft} onClick={() => draft && loadAdvice(draft)}>
              {t("dota.window.refresh")}
            </button>
          </div>
          {suggestError && <p className="muted small">{suggestError}</p>}
          {!suggestError && suggestions && suggestions.length === 0 && <p className="muted small">{t("dota.window.noSuggestions")}</p>}
          {suggestions && suggestions.length > 0 && (
            <ol className="dota-suggestions">
              {suggestions.map((s) => (
                <li key={s.hero_id}>
                  <strong>{s.hero}</strong>
                  <ul className="dota-reasons small">
                    {s.reasons.map((r, i) => (
                      <ReasonLine key={i} reason={r} />
                    ))}
                  </ul>
                </li>
              ))}
            </ol>
          )}
        </section>
      )}

      {tab === "items" && live && <LivePanel live={live} />}

      {tab === "items" && (
        <section className="dota-section">
          <div className="dota-row">
            <h3 className="dota-heading">{plan ? t("dota.window.buildFor", { hero: plan.hero }) : t("dota.window.build")}</h3>
            <button className="link small push-right" disabled={busy || !draft} onClick={() => draft && loadAdvice(draft)}>
              {t("dota.window.refresh")}
            </button>
          </div>
          {draft && draft.player_hero_id == null && <p className="muted small">{t("dota.window.noHero")}</p>}
          {planError && <p className="muted small">{planError}</p>}
          {plan && (
            <>
              <ItemList title={t("dota.window.starting")} items={plan.starting} />
              <ItemList title={t("dota.window.core")} items={plan.core} />
              <ItemList title={t("dota.window.situational")} items={plan.situational} />
              {plan.adaptations.length > 0 && (
                <>
                  <h4 className="dota-subheading">{t("dota.window.adaptations")}</h4>
                  <ul className="dota-reasons small">
                    {plan.adaptations.map((r, i) => (
                      <ReasonLine key={i} reason={r} />
                    ))}
                  </ul>
                </>
              )}
              {plan.data_note && <p className="muted small">{plan.data_note}</p>}
            </>
          )}
          {!live && <p className="muted small">{t("dota.live.hint")}</p>}
        </section>
      )}

      {tab === "lookup" && <LookupTab heroes={heroes} heroError={heroError} />}
      {tab === "review" && <ReviewTab />}

      <footer className="dota-footer muted small">{t("dota.window.disclaimer")}</footer>
    </div>
  );
}
