//! LocalFlow desktop app: a Tauri shell around `localflow-core`.
//!
//! - The window can be closed; LocalFlow keeps running in the system tray.
//! - Scheduled automations keep running while the window is hidden.
//! - `notify()` in scripts shows a native desktop notification.

mod commands;
mod i18n;
mod settings;
mod tray;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};

use localflow_core::{CoreConfig, CoreEvent, LocalFlow};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_notification::NotificationExt;

/// Name of the event the frontend listens to for live updates.
pub const EVENT_NAME: &str = "localflow://event";

/// Passed by autostart so LocalFlow starts quietly in the tray.
pub const MINIMIZED_FLAG: &str = "--minimized";

/// Preferences the Rust side needs while running.
pub struct Prefs {
    pub notifications: AtomicBool,
    /// Resolved language code ("en", "ru", "de") for the tray menu and notifications.
    pub language: RwLock<&'static str>,
}

impl Prefs {
    pub fn texts(&self) -> &'static i18n::Texts {
        i18n::texts(*self.language.read().expect("language lock"))
    }
}

pub struct AppState {
    pub flow: LocalFlow,
    pub prefs: Arc<Prefs>,
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!("could not show notification: {e}");
    }
}

/// Forward core events to the UI, and turn some of them into desktop notifications.
fn handle_event(app: &AppHandle, prefs: &Prefs, event: CoreEvent) {
    if prefs.notifications.load(Ordering::Relaxed) {
        let texts = prefs.texts();
        match &event {
            CoreEvent::Log { level, message, automation_id: Some(_), .. } if level == "notify" => {
                notify(app, "LocalFlow", message);
            }
            // Runs nobody is watching: report failures, and confirm runs started from the tray.
            CoreEvent::RunFinished { name, trigger, run, .. } if trigger != "manual" => {
                if run.status == "failed" {
                    let error = run.error.as_deref().unwrap_or(texts.unknown_error);
                    notify(app, &texts.failed.replace("{name}", name), error);
                } else if trigger == tray::TRAY_TRIGGER {
                    notify(app, "LocalFlow", &texts.finished.replace("{name}", name));
                }
            }
            _ => {}
        }
    }
    if matches!(event, CoreEvent::AutomationsChanged) {
        tray::refresh(app);
    }
    let _ = app.emit(EVENT_NAME, event);
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "localflow_core=info,localflow_desktop_lib=info".into()),
        )
        .init();

    tauri::Builder::default()
        // A second launch just brings the existing window forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main_window(app)))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![MINIMIZED_FLAG]),
        ))
        .setup(|app| {
            let handle = app.handle().clone();

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("localflow.db");
            let config = CoreConfig::new(format!(
                "sqlite://{}",
                db_path.to_string_lossy().replace('\\', "/")
            ));

            let prefs = Arc::new(Prefs {
                notifications: AtomicBool::new(true),
                language: RwLock::new(i18n::resolve("auto")),
            });
            let events_prefs = prefs.clone();
            let events_handle = handle.clone();
            let on_event = Arc::new(move |event| handle_event(&events_handle, &events_prefs, event));

            let (flow, theme) = tauri::async_runtime::block_on(async {
                let flow = LocalFlow::open(config, Some(on_event)).await?;
                let theme = settings::apply_saved(&flow, &prefs).await?;
                flow.start().await?;
                Ok::<_, localflow_core::CoreError>((flow, theme))
            })?;

            tracing::info!("database: {}", db_path.display());
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_theme(settings::window_theme(&theme));
            }
            app.manage(AppState { flow, prefs });
            tray::create(&handle)?;

            if std::env::args().any(|a| a == MINIMIZED_FLAG) {
                if let Some(window) = app.get_webview_window("main") {
                    window.hide()?;
                }
            }
            Ok(())
        })
        // Closing the window hides it; "Quit" in the tray menu exits.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_automations,
            commands::get_automation,
            commands::create_automation,
            commands::update_automation,
            commands::set_enabled,
            commands::delete_automation,
            commands::run_automation,
            commands::test_run,
            commands::validate_code,
            commands::validate_schedule,
            commands::list_runs,
            commands::list_logs,
            commands::get_templates,
            settings::get_settings,
            settings::set_autostart,
            settings::set_notifications,
            settings::set_allowed_dirs,
            settings::set_language,
            settings::set_theme,
        ])
        .run(tauri::generate_context!())
        .expect("error while running LocalFlow");
}
