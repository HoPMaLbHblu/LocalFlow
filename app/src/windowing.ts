// The app window and the separate guide and AI chat windows, and how they talk to each other.

import { invoke } from "@tauri-apps/api/core";
import { emit, emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";

const OPEN_DRAFT = "localflow://open-draft";
const PREFS_CHANGED = "localflow://prefs-changed";
const AI_CONTEXT = "localflow://ai-context";
const AI_CONTEXT_REQUEST = "localflow://ai-context-request";
const AI_APPLY_CODE = "localflow://ai-apply-code";

/** True inside the real desktop app (not the browser preview with fake data). */
export function inDesktopApp(): boolean {
  return !!(window as any).__TAURI_INTERNALS__?.metadata?.currentWindow;
}

/** True when this page is the guide window. */
export function isGuideWindow(): boolean {
  // In the desktop app the window's label decides; "#guide" is for the browser preview.
  const label = (window as any).__TAURI_INTERNALS__?.metadata?.currentWindow?.label;
  return label ? label === "guide" : window.location.hash === "#guide";
}

/** True when this page is the AI chat window ("#aichat" in the browser preview). */
export function isAiChatWindow(): boolean {
  const label = (window as any).__TAURI_INTERNALS__?.metadata?.currentWindow?.label;
  return label ? label === "aichat" : window.location.hash === "#aichat";
}

export function openAiChatWindow(title: string): Promise<void> {
  return invoke("open_ai_chat", { title });
}

/** The automation open in the editor, as the AI chat window needs it (null: none open). */
export interface AiContext {
  chatId: string;
  name: string;
  code: string;
}

/** From the app window: tell the AI chat window what's in the editor now. */
export function publishAiContext(context: AiContext | null): void {
  if (inDesktopApp()) emitTo("aichat", AI_CONTEXT, context).catch(() => {});
}

export function onAiContext(handler: (context: AiContext | null) => void): Promise<UnlistenFn> {
  return listen<AiContext | null>(AI_CONTEXT, (e) => handler(e.payload));
}

/** From the AI chat window: ask the app window to send what's in the editor. */
export function requestAiContext(): void {
  if (inDesktopApp()) emitTo("main", AI_CONTEXT_REQUEST).catch(() => {});
}

export function onAiContextRequest(handler: () => void): Promise<UnlistenFn> {
  return listen(AI_CONTEXT_REQUEST, () => handler());
}

/** From the AI chat window: put code into that automation's editor. */
export function applyAiCode(chatId: string, code: string): Promise<void> {
  return emitTo("main", AI_APPLY_CODE, { chatId, code });
}

export function onAiApplyCode(handler: (change: { chatId: string; code: string }) => void): Promise<UnlistenFn> {
  return listen<{ chatId: string; code: string }>(AI_APPLY_CODE, (e) => handler(e.payload));
}

export function openGuideWindow(title: string): Promise<void> {
  return invoke("open_guide", { title });
}

/** From the guide window: open example code as a new automation in the app window. */
export async function sendDraftToMain(title: string, code: string): Promise<void> {
  await emitTo("main", OPEN_DRAFT, { title, code });
  await invoke("show_main");
}

export function onOpenDraft(handler: (draft: { title: string; code: string }) => void): Promise<UnlistenFn> {
  return listen<{ title: string; code: string }>(OPEN_DRAFT, (e) => handler(e.payload));
}

/** Tell the other windows that the theme or language changed. */
export function announcePrefsChanged(): void {
  if (inDesktopApp()) emit(PREFS_CHANGED).catch(() => {});
}

export function onPrefsChanged(handler: () => void): Promise<UnlistenFn> {
  return listen(PREFS_CHANGED, () => handler());
}
