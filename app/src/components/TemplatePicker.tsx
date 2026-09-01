import type { Template } from "../api";
import { describeSchedule } from "../format";

const BLANK: Template = {
  slug: "blank",
  title: "",
  description: "",
  schedule: "",
  code: `automation {
    name = "My automation",

    run = function(ctx)
        log("Hello from " .. ctx.name)
    end
}
`,
};

interface Props {
  templates: Template[];
  onPick: (template: Template) => void;
  onCancel: () => void;
}

export default function TemplatePicker({ templates, onPick, onCancel }: Props) {
  return (
    <div className="page">
      <h1>New automation</h1>
      <p className="muted">Pick a starting point. You can change everything afterwards.</p>
      <div className="template-grid">
        <button className="card template-card blank" onClick={() => onPick(BLANK)}>
          <strong>Blank</strong>
          <span className="muted small">An empty automation to fill in yourself.</span>
        </button>
        {templates.map((t) => (
          <button key={t.slug} className="card template-card" onClick={() => onPick(t)}>
            <strong>{t.title}</strong>
            <span className="muted small">{t.description}</span>
            <span className="template-schedule small">{describeSchedule(t.schedule || null)}</span>
          </button>
        ))}
      </div>
      <p>
        <button className="secondary" onClick={onCancel}>
          Cancel
        </button>
      </p>
    </div>
  );
}
