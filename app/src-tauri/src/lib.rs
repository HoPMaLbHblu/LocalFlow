//! LocalFlow desktop app: a Tauri shell around `localflow-core`.
//!
//! - The window can be closed; LocalFlow keeps running in the system tray.
//! - Scheduled automations keep running while the window is hidden.
//! - `notify()` in scripts shows a native desktop notification.

mod commands;
mod ai;
mod bots;
mod updates;
mod links;
mod dota;
mod hotkeys;
mod i18n;
mod safety;
mod settings;
mod sharing;
mod tray;
mod voice;
mod windows;

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

pub(crate) fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!("could not show notification: {e}");
    }
}

/// Forward core events to the UI, and turn some of them into desktop notifications.
fn handle_event(app: &AppHandle, prefs: &Prefs, event: CoreEvent) {
    // Always shown, even with notifications off: remote control must never be silent.
    if let CoreEvent::Notice { message } = &event {
        notify(app, "LocalFlow", message);
        return;
    }
    // dota.show() in a script: open the companion window (it names itself once loaded).
    if matches!(event, CoreEvent::ShowDota) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = windows::open_dota(app, "Dota 2".into()).await {
                tracing::warn!("{e:?}");
            }
        });
        return;
    }
    if prefs.notifications.load(Ordering::Relaxed) {
        let texts = prefs.texts();
        match &event {
            CoreEvent::Log {
                level,
                message,
                automation_id: Some(_),
                ..
            } if level == "notify" => {
                notify(app, "LocalFlow", message);
            }
            // Runs nobody is watching: report failures, and confirm runs started from the tray.
            CoreEvent::RunFinished {
                name, trigger, run, ..
            } if trigger != "manual" => {
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
        hotkeys::refresh(app);
    }
    // The tray's "Stop all running automations" item exists only while something runs. A finished
    // run leaves the running set just after its event, so look again a moment later.
    if matches!(event, CoreEvent::RunStarted { .. } | CoreEvent::RunFinished { .. }) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            tray::refresh(&app);
        });
    }
    let _ = app.emit(EVENT_NAME, event);
}

/// Everything that happens once at startup: database, settings, scheduler, tray.
fn init(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();

    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("localflow.db");
    // Saved AI answers (ai.ask with cache_hours) live next to the database.
    localflow_core::ai::set_cache_file(data_dir.join("ai_cache.json"));
    // The Dota 2 companion's cache, draft and settings.
    dota::init(&data_dir);
    // Link sets and other small data files.
    localflow_core::appdata::set_dir(data_dir.clone());
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
        ai::load(&flow).await;
        bots::load(&flow).await;
        flow.start().await?;
        Ok::<_, localflow_core::CoreError>((flow, theme))
    })?;

    tracing::info!("database: {}", db_path.display());
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_theme(settings::window_theme(&theme));
    }
    app.manage(AppState { flow, prefs });
    // Opened by double-clicking a .localflow file: the frontend picks it up when ready.
    app.manage(sharing::PendingImport(std::sync::Mutex::new(
        sharing::file_argument(std::env::args()),
    )));
    app.manage(hotkeys::Hotkeys::default());
    app.manage(hotkeys::VoiceKey::default());
    // Voice control: settings are loaded now; the session starts after the tray is up.
    app.manage(voice::VoiceManager::new(localflow_core::voice::settings::load()));
    app.manage(updates::UpdateCache::default());
    updates::start(handle.clone());
    tray::create(&handle)?;
    hotkeys::refresh(&handle);
    voice::init(&handle);

    if std::env::args().any(|a| a == MINIMIZED_FLAG) {
        if let Some(window) = app.get_webview_window("main") {
            window.hide()?;
        }
    }
    Ok(())
}

/// Tell the user why LocalFlow couldn't start, and where their data is.
fn show_fatal_error(error: &str) {
    tracing::error!("LocalFlow could not start: {error}");
    // Where Tauri keeps app data: %APPDATA% on Windows, ~/Library/Application Support on a Mac.
    let data_dir = dirs::data_dir()
        .map(|d| d.join("com.hopmalbhblu.localflow").display().to_string())
        .unwrap_or_default();
    rfd::MessageDialog::new()
        .set_title("LocalFlow could not start")
        .set_description(format!(
            "{error}\n\nYour automations are stored in:\n{data_dir}\n\n\
             Please report this at https://github.com/HoPMaLbHblu/LocalFlow/issues"
        ))
        .set_level(rfd::MessageLevel::Error)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
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
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            sharing::open_from_second_instance(app, args)
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| hotkeys::handle(app, shortcut, event.state()))
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![MINIMIZED_FLAG]),
        ))
        .setup(|app| {
            // A startup failure shows a readable message instead of silently closing.
            if let Err(error) = init(app) {
                show_fatal_error(&error.to_string());
                std::process::exit(1);
            }
            Ok(())
        })
        // Closing the main window hides it; "Quit" in the tray menu exits.
        // Other windows (the guide) close normally.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
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
            settings::set_script_timeout,
            sharing::export_automation,
            sharing::preview_import,
            sharing::import_automation,
            sharing::take_pending_import,
            safety::list_trash,
            safety::restore_automation,
            safety::delete_forever,
            safety::list_versions,
            safety::restore_version,
            safety::list_backups,
            safety::backup_now,
            safety::restore_backup,
            safety::open_backups_folder,
            safety::startup_notice,
            safety::get_metrics,
            ai::get_ai_settings,
            ai::set_ai_settings,
            ai::clear_ai_key,
            ai::test_ai,
            ai::clear_ai_cache,
            ai::ai_write_automation,
            links::links_list,
            links::links_trash,
            links::links_save,
            links::links_delete,
            links::links_restore,
            links::links_open,
            links::links_parse,
            links::links_import_bookmarks,
            bots::get_bot_settings,
            updates::update_status,
            updates::dismiss_update,
            updates::get_update_check,
            updates::set_update_check,
            updates::open_release_page,
            bots::set_bot_settings,
            bots::clear_bot,
            bots::find_telegram_chats,
            bots::test_bots,
            dota::dota_get_settings,
            dota::dota_set_settings,
            dota::dota_status,
            dota::dota_install_gsi,
            dota::dota_uninstall_gsi,
            dota::dota_draft,
            dota::dota_capture,
            dota::dota_correct,
            dota::dota_set_hero,
            dota::dota_set_team,
            dota::dota_set_role,
            dota::dota_reset,
            dota::dota_suggest,
            dota::dota_build,
            dota::dota_heroes,
            dota::dota_key_saved,
            dota::dota_set_key,
            dota::dota_live,
            dota::dota_lookup,
            dota::dota_last_match,
            dota::dota_recent_matches,
            dota::dota_parse_account,
            dota::dota_set_account,
            dota::dota_set_live_helper,
            windows::open_guide,
            windows::open_ai_chat,
            windows::open_dota,
            windows::show_main,
            voice::voice_get_settings,
            voice::voice_set_settings,
            voice::voice_status,
            voice::voice_list_devices,
            voice::voice_engines,
            voice::voice_download_model,
            voice::voice_cancel_download,
            voice::voice_remove_model,
            voice::voice_test_microphone,
            voice::voice_set_enabled,
            voice::voice_set_muted,
            voice::voice_press,
            voice::voice_release,
            voice::voice_submit_text,
            voice::voice_answer,
            voice::voice_commands,
            voice::running_automations,
            voice::stop_run,
            voice::stop_all_runs,
        ])
        .build(tauri::generate_context!())
        .expect("error while building LocalFlow")
        .run(|app, event| {
            // macOS hands double-clicked files to the running app as an event, not as arguments.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = &event {
                for url in urls {
                    if let Ok(path) = url.to_file_path() {
                        sharing::open_file(app, path.to_string_lossy().into_owned());
                    }
                }
            }
            // Clicking the Dock icon brings the hidden window back.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = &event {
                show_main_window(app);
            }
            // Release the microphone before the process goes away (never waits more than ~2 s).
            if matches!(event, tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit) {
                voice::shutdown(app);
            }
            let _ = (app, event);
        });
}
