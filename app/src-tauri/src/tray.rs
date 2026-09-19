//! System tray icon (the menu bar on a Mac): keeps LocalFlow reachable while the window is closed,
//! and runs any automation in one click from the "Run" menu.

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

use crate::{show_main_window, AppState};

const TRAY_ID: &str = "main";
const RUN_PREFIX: &str = "run:";

/// Trigger name for runs started from the tray (shown in history, and notified on finish).
pub const TRAY_TRIGGER: &str = "tray";

fn build_menu(app: &AppHandle, automations: &[(i64, String)]) -> tauri::Result<Menu<tauri::Wry>> {
    // Before setup finishes there is no state yet; fall back to English.
    let texts = app
        .try_state::<AppState>()
        .map(|s| s.prefs.texts())
        .unwrap_or_else(|| crate::i18n::texts("en"));
    let open = MenuItem::with_id(app, "open", texts.open, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", texts.quit, true, None::<&str>)?;

    let run = Submenu::with_id(app, "run", texts.run, !automations.is_empty())?;
    for (id, name) in automations {
        run.append(&MenuItem::with_id(
            app,
            format!("{RUN_PREFIX}{id}"),
            name,
            true,
            None::<&str>,
        )?)?;
    }

    Menu::with_items(
        app,
        &[&open, &run, &PredefinedMenuItem::separator(app)?, &quit],
    )
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("LocalFlow")
        .menu(&build_menu(app, &[])?)
        // On a Mac, clicking a menu-bar icon opens its menu (which has "Open").
        // On Windows, a left click opens the window and a right click the menu.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            if id == "open" {
                show_main_window(app);
            } else if id == "quit" {
                app.exit(0);
            } else if let Some(automation_id) = id
                .strip_prefix(RUN_PREFIX)
                .and_then(|n| n.parse::<i64>().ok())
            {
                let flow = app.state::<AppState>().flow.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = flow.run(automation_id, TRAY_TRIGGER).await {
                        tracing::error!(automation_id, "tray run failed: {e}");
                    }
                });
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if !cfg!(target_os = "macos") {
                    show_main_window(tray.app_handle());
                }
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    refresh(app);
    Ok(())
}

/// Rebuild the "Run" menu from the current automations. Call when they change.
pub fn refresh(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let flow = app.state::<AppState>().flow.clone();
        let automations: Vec<(i64, String)> = match flow.list().await {
            Ok(list) => list
                .into_iter()
                .filter(|s| s.automation.enabled)
                .map(|s| (s.automation.id, s.automation.name))
                .collect(),
            Err(e) => {
                tracing::warn!("could not load automations for the tray: {e}");
                return;
            }
        };
        let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), build_menu(&app, &automations))
        else {
            return;
        };
        let _ = tray.set_menu(Some(menu));
    });
}
