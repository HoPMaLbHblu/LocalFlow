//! Scripts never destroy files outright: deleted files, and files about to be
//! overwritten, go to the Windows Recycle Bin, where they can be restored.

use std::path::Path;

/// Tests set this to a folder, so their files don't fill the real Recycle Bin.
const TEST_RECYCLE_DIR: &str = "LOCALFLOW_TEST_RECYCLE_DIR";

/// Move a file or folder to the Recycle Bin. If that isn't possible (for
/// example on some network drives), nothing is deleted and an error is returned.
pub fn recycle(path: &Path) -> Result<(), String> {
    if let Some(dir) = std::env::var_os(TEST_RECYCLE_DIR) {
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut target = dir.join(&name);
        let mut n = 1;
        while target.exists() {
            n += 1;
            target = dir.join(format!("{n}-{name}"));
        }
        return std::fs::rename(path, target).map_err(|e| e.to_string());
    }
    trash::delete(path).map_err(|e| format!("could not move {} to the Recycle Bin (nothing was deleted): {e}", path.display()))
}

/// Call before replacing `path`: the current file, if any, goes to the Recycle Bin.
pub fn keep_old_version(path: &Path) -> Result<(), String> {
    if path.is_file() {
        recycle(path)?;
    }
    Ok(())
}
