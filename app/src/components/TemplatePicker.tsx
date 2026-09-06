import type { Template } from "../api";
import { describeSchedule } from "../format";
import { t } from "../i18n";

const BLANK_CODE = `automation {
    name = "My automation",

    run = function(ctx)
        log("Hello from " .. ctx.name)
    end
}
`;

interface Props {
  templates: Template[];
  onPick: (template: Template) => void;
  onCancel: () => void;
}

export default function TemplatePicker({ templates, onPick, onCancel }: Props) {
  const blank: Template = { slug: "blank", title: "", description: "", schedule: "", code: BLANK_CODE };

  return (
    <div className="page">
      <h1>{t("picker.title")}</h1>
      <p className="muted">{t("picker.intro")}</p>
      <div className="template-grid">
        <button className="card template-card blank" onClick={() => onPick(blank)}>
          <strong>{t("picker.blank")}</strong>
          <span className="muted small">{t("picker.blankDescription")}</span>
        </button>
        {templates.map((template) => (
          <button key={template.slug} className="card template-card" onClick={() => onPick(template)}>
            <strong>{template.title}</strong>
            <span className="muted small">{template.description}</span>
            <span className="template-schedule small">
              {template.watch_path
                ? t("trigger.watching", { path: template.watch_path })
                : template.run_on_startup
                  ? t("trigger.onStartup")
                  : describeSchedule(template.schedule || null)}
            </span>
          </button>
        ))}
      </div>
      <p>
        <button className="secondary" onClick={onCancel}>
          {t("common.cancel")}
        </button>
      </p>
    </div>
  );
}
