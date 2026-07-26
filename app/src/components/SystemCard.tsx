import { useEffect, useState } from "react";
import { api, type MetricSample } from "../api";
import { t } from "../i18n";

const RANGES = [
  { minutes: 60, key: "system.hour" },
  { minutes: 24 * 60, key: "system.day" },
  { minutes: 7 * 24 * 60, key: "system.week" },
] as const;

const SERIES = [
  { name: "cpu", key: "system.cpu", color: "var(--accent)" },
  { name: "memory", key: "system.memory", color: "#a855f7" },
  { name: "battery", key: "system.battery", color: "#16a34a" },
] as const;

type SeriesName = (typeof SERIES)[number]["name"];

const WIDTH = 600;
const HEIGHT = 120;

function points(samples: MetricSample[], name: SeriesName, from: number, to: number): string {
  const span = Math.max(1, to - from);
  return samples
    .filter((s) => s[name] != null)
    .map((s) => {
      const x = ((s.at - from) / span) * WIDTH;
      const y = HEIGHT - (Math.min(100, Math.max(0, s[name] as number)) / 100) * HEIGHT;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}

/** CPU, memory and battery over time, from LocalFlow's own history (kept on this PC). */
export default function SystemCard() {
  const [minutes, setMinutes] = useState<number>(60);
  const [samples, setSamples] = useState<MetricSample[] | null>(null);

  useEffect(() => {
    let alive = true;
    const load = () =>
      api
        .getMetrics(minutes, 150)
        .then((list) => alive && setSamples(list ?? []))
        .catch(() => alive && setSamples([]));
    load();
    const timer = setInterval(load, 60_000);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [minutes]);

  const latest = samples && samples.length > 0 ? samples[samples.length - 1] : null;
  const to = Math.floor(Date.now() / 1000);
  const from = to - minutes * 60;
  const shown = SERIES.filter((s) => s.name !== "battery" || samples?.some((x) => x.battery != null));

  return (
    <section className="card system-card">
      <div className="system-head">
        <h2>{t("system.title")}</h2>
        <div className="segmented small" role="radiogroup" aria-label={t("system.title")}>
          {RANGES.map((r) => (
            <button
              key={r.minutes}
              role="radio"
              aria-checked={minutes === r.minutes}
              className={minutes === r.minutes ? "active" : ""}
              onClick={() => setMinutes(r.minutes)}
            >
              {t(r.key)}
            </button>
          ))}
        </div>
      </div>

      {samples === null ? (
        <p className="muted small">{t("runs.loading")}</p>
      ) : samples.length === 0 ? (
        <p className="muted small">{t("system.empty")}</p>
      ) : (
        <>
          <div className="system-now">
            {shown.map((s) => (
              <span key={s.name}>
                <i className="dot" style={{ background: s.color }} />
                {t(s.key)} <strong>{latest?.[s.name] != null ? `${Math.round(latest[s.name] as number)}%` : "–"}</strong>
              </span>
            ))}
            <span className="muted">
              {t("system.disk")} <strong>{latest ? `${Math.round(latest.disk)}%` : "–"}</strong>
            </span>
          </div>
          <svg className="system-chart" viewBox={`0 0 ${WIDTH} ${HEIGHT}`} preserveAspectRatio="none" role="img" aria-label={t("system.title")}>
            {[25, 50, 75].map((p) => (
              <line key={p} x1={0} x2={WIDTH} y1={HEIGHT - (p / 100) * HEIGHT} y2={HEIGHT - (p / 100) * HEIGHT} className="grid" />
            ))}
            {shown.map((s) => (
              <polyline key={s.name} points={points(samples, s.name, from, to)} fill="none" stroke={s.color} strokeWidth={2} vectorEffect="non-scaling-stroke" />
            ))}
          </svg>
        </>
      )}
      <p className="muted small">{t("system.privacy")}</p>
    </section>
  );
}
