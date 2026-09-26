//! Settings › Dota 2 companion and the Dota 2 window. Everything here calls the same core
//! code as the `dota.*` functions in scripts. Slow work (screenshots, statistics) runs off
//! the main thread.

use std::time::Duration;

use localflow_core::{
    dota::{self, launch, DotaSettings, Hero, HeroSuggestion, ItemPlan, Role},
    lua::dota_api::{self, CaptureReport, DraftView, Status},
};
use serde::Serialize;

use crate::commands::CommandError;

fn error(message: impl std::fmt::Display) -> CommandError {
    CommandError::Error { message: message.to_string() }
}

/// Run blocking companion work on a worker thread.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, CommandError> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(error)?.map_err(error)
}

/// Where the companion keeps its files: `<app data>/dota`. Called once at startup.
pub fn init(app_data: &std::path::Path) {
    dota::set_data_dir(app_data.join("dota"));
    dota::data::set_api_key(key_entry().ok().and_then(|e| e.get_password().ok()));
}

/// The optional OpenDota key (raises its request limits) lives in the system's password
/// store, never in LocalFlow's files; scripts can't read it.
fn key_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new("LocalFlow", "OpenDota API key").map_err(|e| e.to_string())
}

/// Whether a key is saved (the key itself never goes to the window).
#[tauri::command]
pub fn dota_key_saved() -> bool {
    key_entry().ok().and_then(|e| e.get_password().ok()).is_some_and(|k| !k.trim().is_empty())
}

/// Save the key, or forget it with `None`/"".
#[tauri::command]
pub fn dota_set_key(key: Option<String>) -> Result<(), CommandError> {
    let entry = key_entry().map_err(error)?;
    match key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
        Some(key) => {
            entry.set_password(&key).map_err(|e| error(format!("Could not save the key in the system's password store: {e}")))?;
            dota::data::set_api_key(Some(key));
        }
        None => {
            let _ = entry.delete_credential();
            dota::data::set_api_key(None);
        }
    }
    Ok(())
}

#[derive(Serialize)]
pub struct DotaSettingsView {
    launch_url: String,
    role: Option<Role>,
    gsi_port: u16,
    launch_assistant: bool,
    /// The .cfg file's text, to copy by hand when LocalFlow can't write it.
    cfg_text: String,
}

#[tauri::command]
pub async fn dota_get_settings() -> Result<DotaSettingsView, CommandError> {
    blocking(|| {
        let s = DotaSettings::load();
        Ok(DotaSettingsView {
            cfg_text: launch::gsi_config_text(s.gsi_port),
            launch_url: s.launch_url,
            role: s.role,
            gsi_port: s.gsi_port,
            launch_assistant: launch::launch_assistant_enabled(),
        })
    })
    .await
}

#[tauri::command]
pub async fn dota_set_settings(
    launch_url: String,
    role: Option<String>,
    gsi_port: u16,
    launch_assistant: bool,
) -> Result<(), CommandError> {
    blocking(move || {
        let url = launch_url.trim().to_string();
        let lower = url.to_lowercase();
        if !url.is_empty() && !(lower.starts_with("https://") || lower.starts_with("http://")) {
            return Err("The page must be a web address starting with https://".into());
        }
        if gsi_port < 1024 {
            return Err("Choose a port between 1024 and 65535.".into());
        }
        let role = match role.as_deref().map(str::trim).filter(|r| !r.is_empty() && *r != "any") {
            Some(r) => Some(Role::parse(r).ok_or(format!("Unknown role \"{r}\"."))?),
            None => None,
        };
        let mut settings = DotaSettings::load();
        let port_changed = settings.gsi_port != gsi_port;
        settings.launch_url = url;
        settings.gsi_port = gsi_port;
        settings.save()?;
        dota_api::set_role(role)?;
        launch::set_launch_assistant(launch_assistant)?;
        // A new port needs the file rewritten (the game reads it at start) and a new listener.
        if port_changed && launch::gsi_installed() {
            launch::install_gsi(gsi_port)?;
            launch::ensure_listening();
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn dota_status() -> Result<Status, CommandError> {
    blocking(|| Ok(dota_api::status())).await
}

/// Write the Game State Integration file into the game folder and start listening.
#[tauri::command]
pub async fn dota_install_gsi() -> Result<String, CommandError> {
    blocking(|| {
        let path = launch::install_gsi(DotaSettings::load().gsi_port)?;
        launch::ensure_listening();
        Ok(path.to_string_lossy().into_owned())
    })
    .await
}

#[tauri::command]
pub async fn dota_uninstall_gsi() -> Result<bool, CommandError> {
    blocking(|| {
        let removed = launch::uninstall_gsi()?;
        launch::ensure_listening();
        Ok(removed)
    })
    .await
}

#[tauri::command]
pub async fn dota_draft() -> Result<DraftView, CommandError> {
    blocking(|| Ok(dota_api::current_draft())).await
}

/// "Capture now": the same as `dota.capture_draft()` in a script.
#[tauri::command]
pub async fn dota_capture() -> Result<CaptureReport, CommandError> {
    blocking(|| dota_api::capture_draft(Duration::from_secs(30))).await
}

#[tauri::command]
pub async fn dota_correct(side: String, slot: u8, hero: Option<String>) -> Result<DraftView, CommandError> {
    blocking(move || {
        let side = dota_api::parse_side(&side)?;
        dota_api::correct(side, slot, hero.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn dota_set_hero(hero: Option<String>) -> Result<DraftView, CommandError> {
    blocking(move || dota_api::set_player_hero(hero.as_deref())).await
}

#[tauri::command]
pub async fn dota_set_team(team: String) -> Result<DraftView, CommandError> {
    blocking(move || dota_api::set_team(dota_api::parse_team(&team)?)).await
}

#[tauri::command]
pub async fn dota_set_role(role: Option<String>) -> Result<(), CommandError> {
    blocking(move || {
        let role = match role.as_deref().map(str::trim).filter(|r| !r.is_empty() && *r != "any") {
            Some(r) => Some(Role::parse(r).ok_or(format!("Unknown role \"{r}\"."))?),
            None => None,
        };
        dota_api::set_role(role)
    })
    .await
}

#[tauri::command]
pub async fn dota_reset() -> Result<DraftView, CommandError> {
    blocking(dota_api::reset_draft).await
}

#[tauri::command]
pub async fn dota_suggest(count: Option<usize>) -> Result<Vec<HeroSuggestion>, CommandError> {
    blocking(move || dota_api::suggest(count.unwrap_or(8))).await
}

#[tauri::command]
pub async fn dota_build(hero: Option<String>) -> Result<ItemPlan, CommandError> {
    blocking(move || dota_api::build(hero.as_deref())).await
}

#[tauri::command]
pub async fn dota_heroes() -> Result<Vec<Hero>, CommandError> {
    blocking(|| {
        let mut heroes = dota_api::hero_list(&*dota_api::source())?;
        heroes.sort_by(|a, b| a.localized_name.cmp(&b.localized_name));
        Ok(heroes)
    })
    .await
}

