//! The Links page: named sets of web addresses that open together in a browser.
//! Everything calls the same core code as the `links.*` functions in scripts.

use localflow_core::links::{self, Link, LinkSet};

use crate::commands::CommandError;

fn error(message: impl std::fmt::Display) -> CommandError {
    CommandError::Error { message: message.to_string() }
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, CommandError> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(error)?.map_err(error)
}

#[tauri::command]
pub fn links_list() -> Vec<LinkSet> {
    links::list()
}

#[tauri::command]
pub fn links_trash() -> Vec<LinkSet> {
    links::trash()
}

/// Save a set. `old_name` renames it first (when it changed).
#[tauri::command]
pub async fn links_save(set: LinkSet, old_name: Option<String>) -> Result<LinkSet, CommandError> {
    blocking(move || {
        if let Some(old) = old_name.filter(|o| !o.eq_ignore_ascii_case(&set.name)) {
            if links::get(&old).is_some() {
                links::rename(&old, &set.name)?;
            }
        }
        links::save(set)
    })
    .await
}

#[tauri::command]
pub async fn links_delete(name: String) -> Result<bool, CommandError> {
    blocking(move || links::delete(&name)).await
}

#[tauri::command]
pub async fn links_restore(name: String) -> Result<(), CommandError> {
    blocking(move || links::restore(&name)).await
}

/// Open a saved set; returns how many links were opened.
#[tauri::command]
pub async fn links_open(name: String) -> Result<usize, CommandError> {
    blocking(move || links::open_set(&name)).await
}

/// Addresses from pasted text (one per line, optional titles).
#[tauri::command]
pub fn links_parse(text: String) -> Vec<Link> {
    links::parse_links(&text)
}

#[tauri::command]
pub async fn links_import_bookmarks(folder: String, browser: String) -> Result<Vec<Link>, CommandError> {
    blocking(move || links::import_bookmarks(&folder, &browser)).await
}
