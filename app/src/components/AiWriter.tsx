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

  const write = async () => {
    setBusy(true);
    setError(null);
    try {
      onCode(await api.aiWriteAutomation(text, language()));
      onClose();
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
