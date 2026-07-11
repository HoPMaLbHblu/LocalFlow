import type { AutomationSummary, Template } from "../api";
import { describeSchedule, formatRelative } from "../format";
import StatusBadge from "./StatusBadge";

interface Props {
  automations: AutomationSummary[];
  templates: Template[];
  onSelect: (id: number) => void;
  onTemplate: (template: Template) => void;
  onNew: () => void;
  onGuide: () => void;
}

export default function Home({ automations, templates, onSelect, onTemplate, onNew, onGuide }: Props) {
  if (automations.length === 0) {
    return (
      <div className="page">
        <div className="welcome">
          <img src="/logo.svg" alt="" width={64} height={64} />
          <h1>Welcome to LocalFlow</h1>
          <p className="muted">
            Automate chores on your computer with small Lua scripts. Run them with one click or on a schedule —
            LocalFlow keeps working in the tray when the window is closed.
          </p>
        </div>
        <button className="card learn-card" onClick={onGuide}>
          <span className="learn-icon">📘</span>
          <span>
            <strong>New to coding? Start with the guide</strong>
            <span className="muted small">
              Short lessons teach you enough Lua to write your own automations, with examples you can try in one click.
            </span>
          </span>
        </button>
        <h2>Start from a template</h2>
        <div className="template-grid">
          {templates.map((t) => (
            <button key={t.slug} className="card template-card" onClick={() => onTemplate(t)}>
              <strong>{t.title}</strong>
              <span className="muted small">{t.description}</span>
            </button>
          ))}
        </div>
        <p>
          <button className="link" onClick={onNew}>
            or start from scratch
          </button>
        </p>
      </div>
    );
  }

  const enabled = automations.filter((a) => a.enabled).length;
  const scheduled = automations.filter((a) => a.enabled && a.next_run).length;
  const failing = automations.filter((a) => a.last_run?.status === "failed").length;

  const upcoming = automations
    .filter((a) => a.enabled && a.next_run)
    .sort((a, b) => a.next_run!.localeCompare(b.next_run!))
    .slice(0, 5);

  const recent = automations
    .filter((a) => a.last_run)
    .sort((a, b) => b.last_run!.started_at.localeCompare(a.last_run!.started_at))
    .slice(0, 8);

  return (
    <div className="page">
      <h1>Overview</h1>
      <div className="stats">
        <Stat value={automations.length} label="automations" />
        <Stat value={enabled} label="enabled" />
        <Stat value={scheduled} label="scheduled" />
        <Stat value={failing} label="failing" tone={failing > 0 ? "bad" : undefined} />
      </div>

      <div className="columns">
        <section className="card">
          <h2>Coming up</h2>
          {upcoming.length === 0 && <p className="muted">Nothing scheduled.</p>}
          <ul className="plain-list">
            {upcoming.map((a) => (
              <li key={a.id}>
                <button className="row-button" onClick={() => onSelect(a.id)}>
                  <span>{a.name}</span>
                  <span className="muted small">
                    {formatRelative(a.next_run)} · {describeSchedule(a.schedule)}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </section>

        <section className="card">
          <h2>Recent activity</h2>
          {recent.length === 0 && <p className="muted">Nothing has run yet.</p>}
          <ul className="plain-list">
            {recent.map((a) => (
              <li key={a.id}>
                <button className="row-button" onClick={() => onSelect(a.id)}>
                  <span>
                    <StatusBadge status={a.last_run!.status} /> {a.name}
                  </span>
                  <span className="muted small">{formatRelative(a.last_run!.started_at)}</span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      </div>
    </div>
  );
}

function Stat({ value, label, tone }: { value: number; label: string; tone?: "bad" }) {
  return (
    <div className={`card stat ${tone ?? ""}`}>
      <span className="stat-value">{value}</span>
      <span className="muted">{label}</span>
    </div>
  );
}
