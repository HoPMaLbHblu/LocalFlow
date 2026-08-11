import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { t } from "../i18n";
import { applyAiCode, onAiContext, requestAiContext, type AiContext } from "../windowing";
import AiChat from "./AiChat";

/** The AI chat in its own window. It follows whichever automation is open in the editor. */
export default function AiChatWindow() {
  const [context, setContext] = useState<AiContext | null>(null);

  useEffect(() => {
    document.title = t("ai.chat.windowTitle");
    const unlisten = onAiContext(setContext);
    // The app window answers with what's in the editor right now.
    requestAiContext();
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  if (!context) {
    return (
      <div className="ai-chat-window">
        <p className="muted ai-chat-empty">{t("ai.chat.noAutomation")}</p>
      </div>
    );
  }

  return (
    <div className="ai-chat-window">
      <AiChat
        key={context.chatId}
        chatId={context.chatId}
        title={context.name}
        currentCode={context.code}
        onCode={(code) => {
          applyAiCode(context.chatId, code).catch(() => {});
        }}
        onClose={() => {
          getCurrentWindow().close().catch(() => {});
        }}
      />
    </div>
  );
}
