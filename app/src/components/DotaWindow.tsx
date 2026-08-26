import { useEffect, useRef, useState } from "react";
import {
  api,
  errorMessages,
  onCoreEvent,
  type DotaDraft,
  type DotaEvidence,
  type DotaHero,
  type DotaHeroSuggestion,
  type DotaItemAdvice,
  type DotaItemPlan,
  type DotaReason,
  type DotaRole,
  type DotaSlot,
  type DotaStatus,
  type DotaTeam,
} from "../api";
import { t, type Key } from "../i18n";
import { DOTA_ROLES, ageOf, roleName } from "./DotaCard";

const HERO_LIST_ID = "dota-hero-list";

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

/** The Dota 2 companion in its own window: game state, the draft, suggestions and items. */
export default function DotaWindow() {
  const [status, setStatus] = useState<DotaStatus | null>(null);
  const [draft, setDraft] = useState<DotaDraft | null>(null);
  const [heroes, setHeroes] = useState<DotaHero[]>([]);
  const [heroError, setHeroError] = useState<string | null>(null);
  const [suggestions, setSuggestions] = useState<DotaHeroSuggestion[] | null>(null);
  const [suggestError, setSuggestError] = useState<string | null>(null);
  const [plan, setPlan] = useState<DotaItemPlan | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [capturing, setCapturing] = useState(false);
  const [message, setMessage] = useState<{ tone: "ok" | "error"; lines: string[] } | null>(null);
  const timer = useRef<number | undefined>(undefined);

  const loadHeroes = () =>
    api
      .dotaHeroes()
      .then((list) => {
        setHeroes(list);
        setHeroError(null);
      })
      .catch((e) => setHeroError(errorMessages(e).join(" ")));

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
      {status && !status.gsi_installed && <p className="muted small">{t("dota.window.noGsi")}</p>}
      {heroError && <p className="muted small">{t("dota.window.heroesMissing", { error: heroError })}</p>}
      {message && (
        <div className={`banner dota-banner ${message.tone}`}>
          {message.lines.map((line, i) => (
            <div key={i}>{line}</div>
          ))}
        </div>
      )}

      {draft && (
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

      <section className="dota-section">
        <h3 className="dota-heading">{plan ? t("dota.window.buildFor", { hero: plan.hero }) : t("dota.window.build")}</h3>
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
      </section>

      <footer className="dota-footer muted small">{t("dota.window.disclaimer")}</footer>
    </div>
  );
}
