import { useEffect, useState } from "react";
import { api, type AutomationSummary, type ExtraTriggers } from "../api";
import { t } from "../i18n";

type AfterWhen = "success" | "failure" | "always";

interface Props {
  /** `null` for a new automation. */
  automationId: number | null;
  triggers: ExtraTriggers;
  allowSystem: boolean;
  onTriggers: (triggers: ExtraTriggers) => void;
  onAllowSystem: (allowed: boolean) => void;
}

const hasAny = (tr: ExtraTriggers) =>
  !!(tr.hotkey || tr.app_start || tr.app_exit || tr.idle_minutes || tr.usb || tr.after?.automation_id);

/** Hotkey, app, idle and USB triggers, and the "Allow system control" permission. */
export default function MoreTriggers({ automationId, triggers, allowSystem, onTriggers, onAllowSystem }: Props) {
  const set = (patch: Partial<ExtraTriggers>) => onTriggers({ ...triggers, ...patch });
  const [others, setOthers] = useState<AutomationSummary[]>([]);
  useEffect(() => {
    api
      .listAutomations()
      .then((list) => setOthers((list ?? []).filter((a) => a.id !== automationId)))
      .catch(() => {});
  }, [automationId]);
  const after = triggers.after;
  const setAfter = (automation_id: number, when: AfterWhen) =>
    set({ after: automation_id ? { automation_id, when } : null });

  return (
    <details className="more-triggers" open={hasAny(triggers) || allowSystem}>
      <summary>{t("more.title")}</summary>

      <div className="more-grid">
        <label>
          {t("more.hotkey")}
          <input
            className="mono"
            value={triggers.hotkey ?? ""}
            placeholder="Ctrl+Alt+K"
            onChange={(e) => set({ hotkey: e.target.value })}
          />
        </label>
        <label>
          {t("more.appStart")}
          <input
            className="mono"
            value={triggers.app_start ?? ""}
            placeholder="steam"
            onChange={(e) => set({ app_start: e.target.value })}
          />
        </label>
        <label>
          {t("more.appExit")}
          <input
            className="mono"
            value={triggers.app_exit ?? ""}
            placeholder="code"
            onChange={(e) => set({ app_exit: e.target.value })}
          />
        </label>
        <label>
          {t("more.idle")}
          <input
            type="number"
            min={1}
            max={1440}
            value={triggers.idle_minutes ?? ""}
            placeholder="10"
            onChange={(e) => set({ idle_minutes: e.target.value ? Number(e.target.value) : null })}
          />
        </label>
      </div>
      <label className="check">
        <input type="checkbox" checked={!!triggers.usb} onChange={(e) => set({ usb: e.target.checked })} />
        <span>
          {t("more.usb")} <span className="muted small">{t("more.usbHint")}</span>
        </span>
      </label>
      <div className="after-row">
        <label>
          {t("more.after")}
          <select
            value={after?.automation_id ?? 0}
            onChange={(e) => setAfter(Number(e.target.value), after?.when ?? "success")}
          >
            <option value={0}>{t("more.afterNone")}</option>
            {others.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
            ))}
          </select>
        </label>
        {after?.automation_id ? (
          <label>
            {t("more.afterWhen")}
            <select value={after.when} onChange={(e) => setAfter(after.automation_id, e.target.value as AfterWhen)}>
              <option value="success">{t("more.afterSuccess")}</option>
              <option value="failure">{t("more.afterFailure")}</option>
              <option value="always">{t("more.afterAlways")}</option>
            </select>
          </label>
        ) : null}
      </div>
      <p className="muted small">{t("more.hint")}</p>

      <label className={`check system-control ${allowSystem ? "on" : ""}`}>
        <input type="checkbox" checked={allowSystem} onChange={(e) => onAllowSystem(e.target.checked)} />
        <span>
          <strong>{t("more.allowSystem")}</strong>
          <span className="muted small"> {t("more.allowSystemHint")}</span>
        </span>
      </label>
    </details>
  );
}
