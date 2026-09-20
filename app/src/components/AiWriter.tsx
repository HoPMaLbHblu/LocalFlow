import { useState } from "react";
import { api, errorMessages } from "../api";
import { language, t } from "../i18n";

interface Props {
  /** Put the written code into the editor. */
  onCode: (code: string) => void;
  onClose: () => void;
}

/** "Write with AI": describe an automation in plain words, get Lua code to review. */
export default function AiWriter({ onCode, onClose }: Props) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [systemNote, setSystemNote] = useState(false);

  const write = async () => {
    setBusy(true);
    setError(null);
    setWarnings([]);
    setSystemNote(false);
    try {
      const result = await api.aiWriteAutomation(text, language());
      onCode(result.code);
      // Problems the checks couldn't get the AI to fix: keep the panel open and show them.
      // Keep the panel open when there is something to tell: problems left, or system control needed.
      setWarnings(result.warnings);
      setSystemNote(result.needs_system_control);
      if (result.warnings.length === 0 && !result.needs_system_control) onClose();
    } catch (e) {
      setError(errorMessages(e).join(" "));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="ai-writer card">
      <label htmlFor="ai-writer-text">
        <strong>{t("ai.writeTitle")}</strong>
        <span className="muted small"> {t("ai.writeHint")}</span>
      </label>
      <textarea
        id="ai-writer-text"
        rows={3}
        value={text}
        placeholder={t("ai.writePlaceholder")}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && text.trim() && !busy) write();
        }}
      />
      {error && <div className="banner error">{error}</div>}
      {warnings.length > 0 && (
        <div className="banner error">
          {t("ai.stillWrong")}
          <ul>
            {warnings.map((w) => (
              <li key={w}>
                <code>{w}</code>
              </li>
            ))}
          </ul>
        </div>
      )}
      {systemNote && <div className="banner">{t("ai.needsSystem")}</div>}
      <div className="actions">
        <button className="primary" disabled={busy || !text.trim()} onClick={write}>
          {busy ? t("ai.writing") : t("ai.write")}
        </button>
        <button className="secondary" disabled={busy} onClick={onClose}>
          {t("ai.cancel")}
        </button>
        <span className="muted small">{t("ai.writeNote")}</span>
      </div>
    </div>
  );
}
