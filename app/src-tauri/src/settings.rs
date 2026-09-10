//! User preferences, stored in the `settings` table.

use std::{path::PathBuf, sync::atomic::Ordering};

use localflow_core::{CoreResult, LocalFlow};
use serde::Serialize;
use tauri::{AppHandle, Manager, State, Theme};
use tauri_plugin_autostart::ManagerExt;

use crate::{commands::CommandError, i18n, tray, AppState, Prefs};

const NOTIFICATIONS: &str = "notifications";
const ALLOWED_DIRS: &str = "allowed_dirs";
const LANGUAGE: &str = "language";
const THEME: &str = "theme";
const SCRIPT_TIMEOUT: &str = "script_timeout_secs";

/// Allowed script time limits, in seconds.
const TIMEOUT_RANGE: std::ops::RangeInclusive<u64> = 10..=3600;

const THEMES: [&str; 3] = ["system", "light", "dark"];

#[derive(Serialize)]
pub struct Settings {
    autostart: bool,
    notifications: bool,
    allowed_dirs: Vec<String>,
    /// "auto", "en", "ru" or "de".
    language: String,
    /// "system", "light" or "dark".
    theme: String,
    /// How long a script may run, in seconds.
    script_timeout_secs: u64,
    data_dir: String,
    version: String,
}

/// Load saved preferences into the running app. Called once at startup.
/// Returns the saved theme so the window can use it straight away.
pub async fn apply_saved(flow: &LocalFlow, prefs: &Prefs) -> CoreResult<String> {
    let repo = flow.repo();
    if let Some(value) = repo.get_setting(NOTIFICATIONS).await? {
        prefs
            .notifications
            .store(value == "true", Ordering::Relaxed);
    }
    if let Some(value) = repo.get_setting(ALLOWED_DIRS).await? {
        let dirs = parse_dirs(&value);
        if !dirs.is_empty() {
            flow.set_allowed_dirs(&dirs);
        }
    }
    if let Some(secs) = repo
        .get_setting(SCRIPT_TIMEOUT)
        .await?
        .and_then(|v| v.parse::<u64>().ok())
    {
        if TIMEOUT_RANGE.contains(&secs) {
            flow.set_script_timeout(std::time::Duration::from_secs(secs));
        }
    }
    let language = repo
        .get_setting(LANGUAGE)
        .await?
        .unwrap_or_else(|| "auto".into());
    *prefs.language.write().expect("language lock") = i18n::resolve(&language);

    Ok(repo
        .get_setting(THEME)
        .await?
        .unwrap_or_else(|| "system".into()))
}

/// The window's title bar theme for a theme setting; `None` follows Windows.
pub fn window_theme(theme: &str) -> Option<Theme> {
    match theme {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        _ => None,
    }
}

/// Stored as one folder per line.
fn parse_dirs(value: &str) -> Vec<PathBuf> {
    value
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn error(message: impl ToString) -> CommandError {
    CommandError::Error {
        message: message.to_string(),
    }
}

#[tauri::command]
pub async fn get_settings(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Settings, CommandError> {
    let repo = state.flow.repo();
    let allowed_dirs = state
        .flow
        .path_policy()
        .roots()
        .iter()
        .map(|p| p.to_string_lossy().trim_start_matches(r"\\?\").to_string())
        .collect();

    Ok(Settings {
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        notifications: state.prefs.notifications.load(Ordering::Relaxed),
        allowed_dirs,
        language: repo
            .get_setting(LANGUAGE)
            .await
            .map_err(error)?
            .unwrap_or_else(|| "auto".into()),
        theme: repo
            .get_setting(THEME)
            .await
            .map_err(error)?
            .unwrap_or_else(|| "system".into()),
        script_timeout_secs: state.flow.script_timeout().as_secs(),
        data_dir: app
            .path()
            .app_data_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        version: app.package_info().version.to_string(),
    })
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), CommandError> {
    let autolaunch = app.autolaunch();
    let result = if enabled {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    };
    result.map_err(error)
}

#[tauri::command]
pub async fn set_notifications(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), CommandError> {
    state.prefs.notifications.store(enabled, Ordering::Relaxed);
    state
        .flow
        .repo()
        .set_setting(NOTIFICATIONS, if enabled { "true" } else { "false" })
        .await
        .map_err(error)
}

/// `language` is "auto" (follow Windows) or a language code.
#[tauri::command]
pub async fn set_language(
    app: AppHandle,
    state: State<'_, AppState>,
    language: String,
) -> Result<(), CommandError> {
    if language != "auto" && !i18n::LANGUAGES.contains(&language.as_str()) {
        return Err(error(format!("unsupported language: {language}")));
    }
    state
        .flow
        .repo()
        .set_setting(LANGUAGE, &language)
        .await
        .map_err(error)?;
    *state.prefs.language.write().expect("language lock") = i18n::resolve(&language);
    tray::refresh(&app);
    Ok(())
}

#[tauri::command]
pub async fn set_theme(
    app: AppHandle,
    state: State<'_, AppState>,
    theme: String,
) -> Result<(), CommandError> {
    if !THEMES.contains(&theme.as_str()) {
        return Err(error(format!("unsupported theme: {theme}")));
    }
    state
        .flow
        .repo()
        .set_setting(THEME, &theme)
        .await
        .map_err(error)?;
    if let Some(window) = app.get_webview_window("main") {
        window.set_theme(window_theme(&theme)).map_err(error)?;
    }
    Ok(())
}

/// How long a script may run before it is stopped (10 seconds to 1 hour).
#[tauri::command]
pub async fn set_script_timeout(
    state: State<'_, AppState>,
    seconds: u64,
) -> Result<(), CommandError> {
    if !TIMEOUT_RANGE.contains(&seconds) {
        return Err(error(format!(
            "time limit must be between 10 and 3600 seconds, got {seconds}"
        )));
    }
    state
        .flow
        .repo()
        .set_setting(SCRIPT_TIMEOUT, &seconds.to_string())
        .await
        .map_err(error)?;
    state
        .flow
        .set_script_timeout(std::time::Duration::from_secs(seconds));
    Ok(())
}

/// Replace the folders scripts may access. Every folder must exist.
#[tauri::command]
pub async fn set_allowed_dirs(
    state: State<'_, AppState>,
    dirs: Vec<String>,
) -> Result<(), CommandError> {
    let mut paths = Vec::new();
    let mut problems = Vec::new();
    for dir in dirs.iter().map(|d| d.trim()).filter(|d| !d.is_empty()) {
        let path = PathBuf::from(dir);
        if path.is_dir() {
            paths.push(path);
        } else {
            problems.push(format!("Folder not found: {dir}"));
        }
    }
    if paths.is_empty() && problems.is_empty() {
        problems.push("Add at least one folder.".into());
    }
    if !problems.is_empty() {
        return Err(CommandError::Validation { messages: problems });
    }

    let value = paths
        .iter()
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    state
        .flow
        .repo()
        .set_setting(ALLOWED_DIRS, &value)
        .await
        .map_err(error)?;
    state.flow.set_allowed_dirs(&paths);
    Ok(())
}
