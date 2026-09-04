//! Runs automations when a new file appears in a watched folder.
//!
//! Files are only handed over once they have stopped changing for a moment, so
//! a download that is still being written doesn't trigger a run halfway through.

use std::{
    collections::HashMap,
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::{sync::Mutex, task::JoinHandle};

use crate::{
    errors::{CoreError, CoreResult},
    lua::sandbox::wildcard_match,
};

/// How long a file must stay unchanged before the automation runs.
const SETTLE: Duration = Duration::from_millis(1500);
/// The same file won't trigger the same automation twice within this time.
const REPEAT_GUARD: Duration = Duration::from_secs(30);

/// What to do when a file is ready: run automation `id` with this file path.
pub type WatchAction = Arc<dyn Fn(i64, String) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

struct ActiveWatch {
    // Dropping the watcher stops the OS notifications.
    _watcher: RecommendedWatcher,
    task: JoinHandle<()>,
}

impl Drop for ActiveWatch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Default)]
pub struct Watchers {
    active: Mutex<HashMap<i64, ActiveWatch>>,
}

/// Browsers and editors write to these first and rename when done.
fn is_partial_download(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.starts_with("~$")
        || [".crdownload", ".part", ".partial", ".download", ".tmp"]
            .iter()
            .any(|ext| lower.ends_with(ext))
}

impl Watchers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start watching `folder` for automation `id` (replacing any earlier watch),
    /// or just stop watching when `folder` is `None`.
    pub async fn sync(
        &self,
        id: i64,
        folder: Option<PathBuf>,
        pattern: Option<String>,
        action: WatchAction,
    ) -> CoreResult<()> {
        self.remove(id).await;
        let Some(folder) = folder else { return Ok(()) };
        let pattern = pattern.filter(|p| !p.trim().is_empty()).unwrap_or_else(|| "*".into());

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PathBuf>();
        let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
            if let Ok(event) = result {
                if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                    for path in event.paths {
                        let _ = tx.send(path);
                    }
                }
            }
        })
        .map_err(|e| CoreError::Scheduler(format!("could not watch folder: {e}")))?;
        watcher
            .watch(&folder, RecursiveMode::NonRecursive)
            .map_err(|e| CoreError::Validation(vec![format!("Could not watch {}: {e}", folder.display())]))?;

        let task = tokio::spawn(async move {
            let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
            let mut recent: HashMap<PathBuf, Instant> = HashMap::new();
            let mut tick = tokio::time::interval(Duration::from_millis(500));

            loop {
                tokio::select! {
                    changed = rx.recv() => match changed {
                        Some(path) => { pending.insert(path, Instant::now()); }
                        None => break,
                    },
                    _ = tick.tick() => {
                        let ready: Vec<PathBuf> = pending
                            .iter()
                            .filter(|(_, last)| last.elapsed() >= SETTLE)
                            .map(|(path, _)| path.clone())
                            .collect();
                        recent.retain(|_, when| when.elapsed() < REPEAT_GUARD);

                        for path in ready {
                            pending.remove(&path);
                            let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
                            if !path.is_file() || is_partial_download(&name) || !wildcard_match(&pattern, &name) {
                                continue;
                            }
                            if recent.contains_key(&path) {
                                continue;
                            }
                            recent.insert(path.clone(), Instant::now());
                            // One run at a time, in the order files arrived.
                            action(id, path.to_string_lossy().into_owned()).await;
                        }
                    }
                }
            }
        });

        tracing::info!(automation_id = id, folder = %folder.display(), "watching folder");
        self.active.lock().await.insert(id, ActiveWatch { _watcher: watcher, task });
        Ok(())
    }

    pub async fn remove(&self, id: i64) {
        self.active.lock().await.remove(&id);
    }

    pub async fn is_watching(&self, id: i64) -> bool {
        self.active.lock().await.contains_key(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::is_partial_download;

    #[test]
    fn partial_downloads_are_ignored() {
        assert!(is_partial_download("movie.mp4.crdownload"));
        assert!(is_partial_download("setup.exe.part"));
        assert!(is_partial_download("~$report.docx"));
        assert!(!is_partial_download("report.pdf"));
    }
}
