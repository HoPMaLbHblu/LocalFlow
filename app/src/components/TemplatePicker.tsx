import { useState } from "react";
import type { Template } from "../api";
import { describeTriggers } from "../format";
import { t, tMaybe } from "../i18n";

const BLANK_CODE = `automation {
    name = "My automation",

    run = function(ctx)
        log("Hello from " .. ctx.name)
    end
}
`;

/** The order categories are shown in. */
const CATEGORIES = ["start", "files", "photos", "apps", "system", "look", "internet", "phone", "daily", "ai", "games"];

interface Props {
  templates: Template[];
  onPick: (template: Template) => void;
  onCancel: () => void;
}

function categoryName(category: string): string {
  return tMaybe(`picker.category.${category}`) ?? category;
}

export default function TemplatePicker({ templates, onPick, onCancel }: Props) {
  const blank: Template = { slug: "blank", title: "", description: "", schedule: "", code: BLANK_CODE };
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<string | null>(null);

  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const matches = templates.filter((template) => {
    if (category && (template.category || "start") !== category) return false;
    const text = `${template.title} ${template.description} ${template.slug}`.toLowerCase();
    return words.every((w) => text.includes(w));
  });
  const present = CATEGORIES.filter((c) => templates.some((template) => (template.category || "start") === c));
  const groups = present
    .map((c) => ({ category: c, items: matches.filter((template) => (template.category || "start") === c) }))
    .filter((g) => g.items.length > 0);

  const card = (template: Template) => (
    <button key={template.slug} className="card template-card" onClick={() => onPick(template)}>
      <strong>{template.title}</strong>
      <span className="muted small">{template.description}</span>
      <span className="template-schedule small">
        {describeTriggers({
          enabled: true,
          schedule: template.schedule || null,
          run_on_startup: !!template.run_on_startup,
          watch_path: template.watch_path || null,
          watch_pattern: template.watch_pattern || null,
          triggers: template.triggers || null,
        })}
      </span>
    </button>
  );

  return (
    <div className="page">
      <h1>{t("picker.title")}</h1>
      <p className="muted">{t("picker.intro", { n: templates.length })}</p>

      <div className="picker-tools">
        <input
          type="search"
          className="picker-search"
          placeholder={t("picker.search")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          autoFocus
        />
        <div className="picker-chips" role="group" aria-label={t("picker.categories")}>
          <button className={`chip ${category === null ? "active" : ""}`} onClick={() => setCategory(null)}>
            {t("picker.all")}
          </button>
          {present.map((c) => (
            <button key={c} className={`chip ${category === c ? "active" : ""}`} onClick={() => setCategory(category === c ? null : c)}>
              {categoryName(c)}
            </button>
          ))}
        </div>
      </div>

      {!query && !category && (
        <div className="template-grid">
          <button className="card template-card blank" onClick={() => onPick(blank)}>
            <strong>{t("picker.blank")}</strong>
            <span className="muted small">{t("picker.blankDescription")}</span>
          </button>
        </div>
      )}

      {groups.map((g) => (
        <section key={g.category}>
          <h2 className="picker-group">{categoryName(g.category)}</h2>
          <div className="template-grid">{g.items.map(card)}</div>
        </section>
      ))}
      {groups.length === 0 && <p className="muted">{t("picker.noMatches")}</p>}

      <p>
        <button className="secondary" onClick={onCancel}>
          {t("common.cancel")}
        </button>
      </p>
    </div>
  );
}
