// Wording for Macs: the menu bar instead of the taskbar, Cmd instead of Ctrl,
// "Open at login" instead of "Start with Windows", the Trash instead of the Recycle Bin.
// Only the texts that differ are listed; everything else comes from the normal dictionaries.

import type { Key } from "./en";

type Overrides = Partial<Record<Key, string>>;

const en: Overrides = {
  "voice.mic.permission": "LocalFlow is not allowed to use the microphone. Open System Settings > Privacy & Security > Microphone, switch LocalFlow on, then test again.",
  "voice.key.help": "Modifiers plus a key, for example Cmd+Option+Space. It works even when LocalFlow is in the background.",
  "voice.enableHelp": "The microphone is used only while this is on. Switch it off here, in the sidebar or from the menu bar icon and the microphone is released at once.",
  "home.welcomeText":
    "Automate chores on your computer with small Lua scripts. Run them with one click or on a schedule. LocalFlow keeps working in the menu bar (the icons at the top right of the screen) when the window is closed.",
  "view.onStartupHint": "(with Settings › Open at login, that's every time you log in)",
  "console.empty": "Output: press Test run (⌘ Enter) to try your script without saving.",
  "settings.languageAuto": "Automatic (Mac language)",
  "settings.autostart": "Open at login",
  "settings.autostartText":
    "LocalFlow starts quietly in the menu bar (the icons at the top right of the screen) when you log in, so schedules keep running.",
  "settings.trayNote":
    "Closing the window keeps LocalFlow running in the menu bar (the icons at the top right of the screen). Click the LocalFlow icon there to run automations or choose Quit to exit, or press ⌘ Q.",
  "guide.copyFailed": "Copy failed, select the code and press ⌘ C",
  "template.backup-rotation.description": "Keep the 7 newest backups in ~/Backups; older ones go to the Trash.",
  "template.clean-temp-files.description": "Every Sunday, move week-old .tmp, .part and unfinished downloads to the Trash.",
};

const ru: Overrides = {
  "voice.mic.permission": "LocalFlow не разрешено использовать микрофон. Откройте Системные настройки > Конфиденциальность и безопасность > Микрофон, включите LocalFlow и проверьте снова.",
  "voice.key.help": "Модификаторы и клавиша, например Cmd+Option+Space. Работает, даже когда LocalFlow в фоне.",
  "voice.enableHelp": "Микрофон используется только пока переключатель включён. Выключите его здесь, в боковой панели или в значке в строке меню, и микрофон освободится сразу.",
  "home.welcomeText":
    "Автоматизируйте рутину на компьютере с помощью небольших скриптов на Lua. Запускайте их одним щелчком или по расписанию. Когда окно закрыто, LocalFlow продолжает работать в строке меню (значки в правом верхнем углу экрана).",
  "view.onStartupHint": "(вместе с «Настройки › Открывать при входе» это каждый вход в систему)",
  "console.empty": "Вывод: нажмите «Тестовый запуск» (⌘ Enter), чтобы попробовать скрипт без сохранения.",
  "settings.languageAuto": "Автоматически (язык Mac)",
  "settings.autostart": "Открывать при входе",
  "settings.autostartText":
    "При входе в систему LocalFlow тихо запускается в строке меню (значки в правом верхнем углу экрана), чтобы расписания продолжали работать.",
  "settings.trayNote":
    "Если закрыть окно, LocalFlow продолжит работать в строке меню (значки в правом верхнем углу экрана). Щёлкните значок LocalFlow, чтобы запустить автоматизацию или выбрать «Выход», либо нажмите ⌘ Q.",
  "guide.copyFailed": "Не скопировалось — выделите код и нажмите ⌘ C",
  "template.backup-rotation.description": "Оставляет 7 новейших резервных копий в ~/Backups, старые отправляет в Корзину.",
  "template.clean-temp-files.description": "Каждое воскресенье отправляет в Корзину недельные .tmp, .part и недокачанные файлы.",
};

const de: Overrides = {
  "voice.mic.permission": "LocalFlow darf das Mikrofon nicht benutzen. Öffne Systemeinstellungen > Datenschutz & Sicherheit > Mikrofon, schalte LocalFlow ein und teste dann erneut.",
  "voice.key.help": "Zusatztasten plus eine Taste, zum Beispiel Cmd+Option+Leertaste. Funktioniert auch, wenn LocalFlow im Hintergrund ist.",
  "voice.enableHelp": "Das Mikrofon wird nur benutzt, solange dies an ist. Schalte es hier, in der Seitenleiste oder über das Menüleisten-Symbol aus, und das Mikrofon wird sofort freigegeben.",
  "home.welcomeText":
    "Automatisiere Routinearbeiten am Computer mit kleinen Lua-Skripten. Starte sie per Klick oder nach Zeitplan. Wenn das Fenster geschlossen ist, arbeitet LocalFlow in der Menüleiste weiter (die Symbole oben rechts auf dem Bildschirm).",
  "view.onStartupHint": "(zusammen mit „Einstellungen › Bei Anmeldung öffnen“ bei jeder Anmeldung)",
  "console.empty": "Ausgabe: Drücke „Testlauf“ (⌘ Enter), um dein Skript ohne Speichern auszuprobieren.",
  "settings.languageAuto": "Automatisch (Mac-Sprache)",
  "settings.autostart": "Bei Anmeldung öffnen",
  "settings.autostartText":
    "LocalFlow startet bei der Anmeldung leise in der Menüleiste (die Symbole oben rechts auf dem Bildschirm), damit Zeitpläne weiterlaufen.",
  "settings.trayNote":
    "Wenn du das Fenster schließt, läuft LocalFlow in der Menüleiste weiter (die Symbole oben rechts auf dem Bildschirm). Klicke dort auf das LocalFlow-Symbol, um Automatisierungen zu starten oder „Beenden“ zu wählen, oder drücke ⌘ Q.",
  "guide.copyFailed": "Kopieren fehlgeschlagen – Code markieren und ⌘ C drücken",
  "template.backup-rotation.description": "Behält die 7 neuesten Sicherungen in ~/Backups; ältere wandern in den Papierkorb.",
  "template.clean-temp-files.description": "Verschiebt jeden Sonntag eine Woche alte .tmp-, .part- und unfertige Downloads in den Papierkorb.",
};

export const MAC_OVERRIDES: Record<"en" | "ru" | "de", Overrides> = { en, ru, de };

/** True when LocalFlow runs on a Mac. */
export const IS_MAC = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform || navigator.userAgent);

/** The keyboard shortcut text for this computer: "Ctrl+S" or "⌘ S". */
export function shortcut(key: string): string {
  return IS_MAC ? `⌘ ${key}` : `Ctrl+${key}`;
}
