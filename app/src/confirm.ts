import { ask } from "@tauri-apps/plugin-dialog";
import { t } from "./i18n";
import { inDesktopApp } from "./windowing";

/**
 * Ask the user to confirm something (delete forever, restore a backup, discard
 * changes, ...). Resolves to true only if they said yes.
 *
 * Inside the desktop window `window.confirm` does not show anything and simply
 * returns true, so it must never be used there: this shows a real Windows
 * dialog instead. Anything going wrong counts as "no".
 */
export async function confirmAction(message: string): Promise<boolean> {
  if (!inDesktopApp()) return window.confirm(message);
  try {
    return await ask(message, {
      title: "LocalFlow",
      kind: "warning",
      okLabel: t("common.yes"),
      cancelLabel: t("common.cancel"),
    });
  } catch {
    return false;
  }
}
