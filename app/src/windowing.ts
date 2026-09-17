// The app window and the separate guide window, and how they talk to each other.

import { invoke } from "@tauri-apps/api/core";
import { emit, emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";

const OPEN_DRAFT = "localflow://open-draft";
const PREFS_CHANGED = "localflow://prefs-changed";

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
