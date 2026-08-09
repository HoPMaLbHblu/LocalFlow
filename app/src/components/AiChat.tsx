import { useEffect, useRef, useState } from "react";
import { api, errorMessages } from "../api";
import { language, t } from "../i18n";

/** One message in the chat. */
interface Message {
  from: "user" | "ai";
  text: string;
  /** For AI replies that changed the code: the code, and what's still wrong with it. */
  code?: string;
  warnings?: string[];
  needsSystemControl?: boolean;
  error?: boolean;
}

interface Props {
  /** Which automation this chat belongs to ("draft" for a new one); each keeps its own dialogue. */
  chatId: string;
  /** The code in the editor now. */
  currentCode: string;
  /** Put the AI's code into the editor. */
  onCode: (code: string) => void;
  onClose: () => void;
}

const MAX_SAVED = 60;

function storageKey(chatId: string) {
  return `localflow.aichat.${chatId}`;
}

function loadMessages(chatId: string): Message[] {
  try {
    const saved = localStorage.getItem(storageKey(chatId));
    return saved ? (JSON.parse(saved) as Message[]) : [];
  } catch {
    return [];
  }
}

function saveMessages(chatId: string, messages: Message[]) {
  try {
    localStorage.setItem(storageKey(chatId), JSON.stringify(messages.slice(-MAX_SAVED)));
  } catch {
    // Only a convenience; the chat still works without it.
  }
}

/** The AI chat docked next to the editor. It remembers the whole dialogue for each automation. */
export default function AiChat({ chatId, currentCode, onCode, onClose }: Props) {
  const [messages, setMessages] = useState<Message[]>(() => loadMessages(chatId));
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const bottom = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    saveMessages(chatId, messages);
  }, [chatId, messages]);
  // Keep the newest message in view; only the message list scrolls, the panel stays put.
  useEffect(() => {
    const list = bottom.current?.parentElement;
    if (list) list.scrollTop = list.scrollHeight;
  }, [messages, busy]);

  /** Earlier turns as the AI saw them: the request, and the code or the answer it gave. */
  const history = () => {
    const turns: { request: string; reply: string }[] = [];
    messages.forEach((m, i) => {
      const next = messages[i + 1];
      if (m.from === "user" && next?.from === "ai" && !next.error) {
        turns.push({ request: m.text, reply: next.code ?? `ANSWER: ${next.text}` });
      }
    });
    return turns;
  };

  const send = async () => {
    const request = text.trim();
    if (!request || busy) return;
    setText("");
    setMessages((m) => [...m, { from: "user", text: request }]);
    setBusy(true);
    try {
      const result = await api.aiWriteAutomation(request, language(), currentCode.trim() ? currentCode : null, history());
      if (result.answer) {
        setMessages((m) => [...m, { from: "ai", text: result.answer! }]);
      } else {
        onCode(result.code);
        setMessages((m) => [
          ...m,
          {
            from: "ai",
            text: result.warnings.length ? t("ai.chat.codeWithProblems") : t("ai.chat.codeUpdated"),
            code: result.code,
            warnings: result.warnings,
            needsSystemControl: result.needs_system_control,
          },
        ]);
      }
    } catch (e) {
      setMessages((m) => [...m, { from: "ai", text: errorMessages(e).join(" "), error: true }]);
    } finally {
      setBusy(false);
    }
  };

  return (
    <aside className="ai-chat" aria-label={t("ai.chat.title")}>
      <div className="help-panel-header">
        <strong className="small">{t("ai.chat.title")}</strong>
        {messages.length > 0 && (
          <button className="link small push-right" disabled={busy} onClick={() => setMessages([])}>
            {t("ai.chat.new")}
          </button>
        )}
        <button className={`link small ${messages.length ? "" : "push-right"}`} onClick={onClose} title={t("help.close")}>
          ✕
        </button>
      </div>

      <div className="ai-chat-messages" aria-live="polite">
        {messages.length === 0 && <p className="muted small ai-chat-empty">{t("ai.chat.empty")}</p>}
        {messages.map((m, i) => (
          <div key={i} className={`ai-msg ${m.from}${m.error ? " error" : ""}`}>
            <div className="ai-msg-text">{m.text}</div>
            {m.needsSystemControl && <div className="ai-msg-note small">{t("ai.needsSystem")}</div>}
            {m.warnings && m.warnings.length > 0 && (
              <ul className="ai-msg-warnings small">
                {m.warnings.map((w) => (
                  <li key={w}>{w}</li>
                ))}
              </ul>
            )}
            {m.code && (
              <details className="ai-msg-code">
                <summary className="small">{t("ai.chat.showCode")}</summary>
                <pre>{m.code}</pre>
                <button className="link small" onClick={() => onCode(m.code!)}>
                  {t("ai.chat.useThis")}
                </button>
              </details>
            )}
          </div>
        ))}
        {busy && <div className="ai-msg ai muted small">{t("ai.writing")}</div>}
        <div ref={bottom} />
      </div>

      <div className="ai-chat-input">
        <textarea
          id="ai-chat-text"
          rows={2}
          value={text}
          placeholder={t("ai.chat.placeholder")}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            // Enter sends, Shift+Enter makes a new line.
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
        />
        <button className="primary" disabled={busy || !text.trim()} onClick={send}>
          {t("ai.chat.send")}
        </button>
      </div>
      <p className="muted small ai-chat-note">{t("ai.writeNote")}</p>
    </aside>
  );
}
