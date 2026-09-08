//! More file functions: reading and writing text, searching folders, finding
//! large files, hashes, zip archives, and a name-based suspicious-file check.
//!
//! ```lua
//! fs.read(path)                    -- a text file's contents
//! fs.write(path, text)             -- create or replace a text file
//! fs.append(path, text)            -- add to the end of a text file
//! fs.rename(path, new_name)        -- rename in the same folder; returns the new path
//! fs.list_dirs(folder)             -- the folders inside a folder
//! fs.find(folder, pattern)         -- files matching pattern, in all subfolders too
//! fs.largest(folder, count)        -- the biggest files, as { path = ..., size = ... }
//! fs.hash(path)                    -- SHA-256 of a file, as hex text
//! zip.create(zip_path, source)     -- zip a file, a folder, or a list of them
//! zip.extract(zip_path, folder)    -- unpack a zip archive
//! security.scan(folder, options)   -- files whose names look like malware
//! ```
//!
//! Searches stop when the script's time limit is reached; they then return what
//! they found so far, plus `false` as a second value.

use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use mlua::{Lua, Table, Value};
use sha2::{Digest, Sha256};

use super::sandbox::{wildcard_match, PathPolicy};

/// Largest text file `fs.read` will load.
const MAX_READ_BYTES: u64 = 10 * 1024 * 1024;
/// Upper bound on results from a search, to keep memory in check.
const MAX_RESULTS: usize = 10_000;

fn err(function: &str, error: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(format!("{function}: {error}"))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Visit every file below `root` (not following folder links), until `visit`
/// returns false or the deadline passes. Unreadable folders are skipped.
/// Returns true if the whole tree was visited.
fn walk_files(root: &Path, deadline: Instant, mut visit: impl FnMut(&Path, &std::fs::Metadata) -> bool) -> bool {
    let mut stack = vec![root.to_path_buf()];
    let mut seen = 0u32;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            seen += 1;
            if seen % 256 == 0 && Instant::now() >= deadline {
                return false;
            }
            let Ok(file_type) = entry.file_type() else { continue };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                let Ok(meta) = entry.metadata() else { continue };
                if !visit(&entry.path(), &meta) {
                    return true;
                }
            }
        }
    }
    true
}

pub fn register(lua: &Lua, fs: &Table, policy: Arc<PathPolicy>, deadline: Instant) -> mlua::Result<()> {
    let p = policy.clone();
    fs.set(
        "read",
        lua.create_function(move |lua, path: String| {
            let resolved = p.resolve(&path).map_err(|e| err("fs.read", e))?;
            let meta = std::fs::metadata(&resolved).map_err(|_| err("fs.read", format!("file not found: {path}")))?;
            if meta.len() > MAX_READ_BYTES {
                return Err(err("fs.read", "file is larger than 10 MB"));
            }
            let bytes = std::fs::read(&resolved).map_err(|e| err("fs.read", e))?;
            lua.create_string(&bytes)
        })?,
    )?;

    let p = policy.clone();
    fs.set(
        "write",
        lua.create_function(move |_, (path, text): (String, mlua::String)| {
            let resolved = p.resolve(&path).map_err(|e| err("fs.write", e))?;
            if let Some(parent) = resolved.parent() {
                std::fs::create_dir_all(parent).map_err(|e| err("fs.write", e))?;
            }
            std::fs::write(&resolved, text.as_bytes()).map_err(|e| err("fs.write", e))?;
            Ok(path_string(&resolved))
        })?,
    )?;

    let p = policy.clone();
    fs.set(
        "append",
        lua.create_function(move |_, (path, text): (String, mlua::String)| {
            let resolved = p.resolve(&path).map_err(|e| err("fs.append", e))?;
            if let Some(parent) = resolved.parent() {
                std::fs::create_dir_all(parent).map_err(|e| err("fs.append", e))?;
            }
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&resolved)
                .map_err(|e| err("fs.append", e))?;
            file.write_all(&text.as_bytes()).map_err(|e| err("fs.append", e))?;
            Ok(path_string(&resolved))
        })?,
    )?;

    let p = policy.clone();
    fs.set(
        "rename",
        lua.create_function(move |_, (path, new_name): (String, String)| {
            if new_name.is_empty() || new_name.contains(['/', '\\']) || new_name == "." || new_name == ".." {
                return Err(err("fs.rename", "the new name must be a plain file name, without folders"));
            }
            let resolved = p.resolve(&path).map_err(|e| err("fs.rename", e))?;
            if !resolved.exists() {
                return Err(err("fs.rename", format!("source not found: {path}")));
            }
            let target = resolved.with_file_name(&new_name);
            if target.exists() {
                return Err(err("fs.rename", format!("destination already exists: {}", target.display())));
            }
            std::fs::rename(&resolved, &target).map_err(|e| err("fs.rename", e))?;
            Ok(path_string(&target))
        })?,
    )?;

    let p = policy.clone();
    fs.set(
        "list_dirs",
        lua.create_function(move |_, path: String| {
            let dir = p.resolve(&path).map_err(|e| err("fs.list_dirs", e))?;
            if !dir.is_dir() {
                return Err(err("fs.list_dirs", format!("directory not found: {path}")));
            }
            let mut dirs: Vec<String> = std::fs::read_dir(&dir)
                .map_err(|e| err("fs.list_dirs", e))?
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| path_string(&e.path()))
                .collect();
            dirs.sort();
            Ok(dirs)
        })?,
    )?;

    let p = policy.clone();
    fs.set(
        "find",
        lua.create_function(move |_, (path, pattern): (String, Option<String>)| {
            let dir = p.resolve(&path).map_err(|e| err("fs.find", e))?;
            if !dir.is_dir() {
                return Err(err("fs.find", format!("directory not found: {path}")));
            }
            let pattern = pattern.unwrap_or_else(|| "*".into());
            let mut found = Vec::new();
            let complete = walk_files(&dir, deadline, |file, _| {
                let name = file.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
                if wildcard_match(&pattern, &name) {
                    found.push(path_string(file));
                }
                found.len() < MAX_RESULTS
            });
            found.sort();
            Ok((found, complete))
        })?,
    )?;

    let p = policy.clone();
    fs.set(
        "largest",
        lua.create_function(move |lua, (path, count): (String, Option<usize>)| {
            let dir = p.resolve(&path).map_err(|e| err("fs.largest", e))?;
            if !dir.is_dir() {
                return Err(err("fs.largest", format!("directory not found: {path}")));
            }
            let count = count.unwrap_or(10).clamp(1, 1000);
            // Keep only the `count` biggest seen so far.
            let mut top: Vec<(u64, PathBuf)> = Vec::with_capacity(count + 1);
            let complete = walk_files(&dir, deadline, |file, meta| {
                let size = meta.len();
                if top.len() < count || size > top.last().map_or(0, |t| t.0) {
                    let at = top.partition_point(|t| t.0 >= size);
                    top.insert(at, (size, file.to_path_buf()));
                    top.truncate(count);
                }
                true
            });
            let results = lua.create_table()?;
            for (size, file) in top {
                let entry = lua.create_table()?;
                entry.set("path", path_string(&file))?;
                entry.set("size", size)?;
                results.push(entry)?;
            }
            Ok((results, complete))
        })?,
    )?;

    let p = policy.clone();
    fs.set(
        "hash",
        lua.create_function(move |_, path: String| {
            let resolved = p.resolve(&path).map_err(|e| err("fs.hash", e))?;
            let mut file = File::open(&resolved).map_err(|_| err("fs.hash", format!("file not found: {path}")))?;
            let mut hasher = Sha256::new();
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                let read = file.read(&mut buffer).map_err(|e| err("fs.hash", e))?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>())
        })?,
    )?;

    lua.globals().set("zip", zip_table(lua, policy.clone())?)?;
    lua.globals().set("security", security_table(lua, policy, deadline)?)?;
    Ok(())
}

// ---- zip -----------------------------------------------------------------------

fn zip_table(lua: &Lua, policy: Arc<PathPolicy>) -> mlua::Result<Table> {
    let zip = lua.create_table()?;

    let p = policy.clone();
    zip.set(
        "create",
        lua.create_function(move |_, (zip_path, source): (String, Value)| {
            let target = p.resolve(&zip_path).map_err(|e| err("zip.create", e))?;
            let sources: Vec<String> = match source {
                Value::String(s) => vec![s.to_str()?.to_string()],
                Value::Table(t) => t.sequence_values::<String>().collect::<mlua::Result<_>>()?,
                _ => return Err(err("zip.create", "source must be a path or a list of paths")),
            };
            let mut inputs = Vec::new();
            for source in &sources {
                let resolved = p.resolve(source).map_err(|e| err("zip.create", e))?;
                if !resolved.exists() {
                    return Err(err("zip.create", format!("source not found: {source}")));
                }
                inputs.push(resolved);
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| err("zip.create", e))?;
            }
            create_zip(&target, &inputs).map_err(|e| err("zip.create", e))
        })?,
    )?;

    zip.set(
        "extract",
        lua.create_function(move |_, (zip_path, folder): (String, String)| {
            let archive = policy.resolve(&zip_path).map_err(|e| err("zip.extract", e))?;
            let destination = policy.resolve(&folder).map_err(|e| err("zip.extract", e))?;
            extract_zip(&archive, &destination).map_err(|e| err("zip.extract", e))
        })?,
    )?;

    Ok(zip)
}

/// Returns the number of files added.
fn create_zip(target: &Path, inputs: &[PathBuf]) -> Result<usize, String> {
    let file = File::create(target).map_err(|e| e.to_string())?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut added = 0;

    let mut add_file = |writer: &mut zip::ZipWriter<File>, path: &Path, name: String| -> Result<(), String> {
        writer.start_file(name, options).map_err(|e| e.to_string())?;
        let mut source = File::open(path).map_err(|e| e.to_string())?;
        std::io::copy(&mut source, writer).map_err(|e| e.to_string())?;
        added += 1;
        Ok(())
    };

    for input in inputs {
        let base = input.parent().unwrap_or(input);
        if input.is_file() {
            let name = input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            add_file(&mut writer, input, name)?;
            continue;
        }
        let mut files = Vec::new();
        walk_files(input, Instant::now() + std::time::Duration::from_secs(3600), |file, _| {
            if file != target {
                files.push(file.to_path_buf());
            }
            true
        });
        files.sort();
        for file in files {
            let relative = file.strip_prefix(base).unwrap_or(&file);
            let name = relative.to_string_lossy().replace('\\', "/");
            add_file(&mut writer, &file, name)?;
        }
    }
    writer.finish().map_err(|e| e.to_string())?;
    Ok(added)
}

/// Returns the number of files extracted. Entries that would land outside
/// `destination` (a "zip slip") are refused.
fn extract_zip(archive: &Path, destination: &Path) -> Result<usize, String> {
    let file = File::open(archive).map_err(|_| format!("zip file not found: {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("not a valid zip file: {e}"))?;
    std::fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    let mut extracted = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(relative) = entry.enclosed_name() else {
            return Err(format!("unsafe path in archive: {}", entry.name()));
        };
        let out = destination.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut target = File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut target).map_err(|e| e.to_string())?;
        extracted += 1;
    }
    Ok(extracted)
}

// ---- suspicious files --------------------------------------------------------------

/// Words that malware names often contain. A match is a hint, not proof.
const SUSPICIOUS_WORDS: &[&str] = &[
    "trojan", "rootkit", "worm", "malware", "virus", "keylogger", "ransomware", "ransom", "backdoor",
    "spyware", "stealer", "exploit", "botnet", "cryptominer", "coinminer", "hacktool", "keygen",
    "injector", "rat_", "_rat", "adware",
];

/// Programs and scripts that run when opened.
const EXECUTABLE_EXTENSIONS: &[&str] = &["exe", "scr", "com", "pif", "bat", "cmd", "vbs", "vbe", "js", "jse", "wsf", "hta", "ps1", "msi", "jar"];

/// Files that can carry running code: a suspicious *word* only matters for these.
/// (A picture called "virus.png" is harmless.)
const CODE_CARRYING_EXTENSIONS: &[&str] = &["dll", "sys", "drv", "ocx", "cpl", "lnk", "zip", "rar", "7z", "iso", "img"];

/// Extensions a harmless-looking file would have.
const DOCUMENT_EXTENSIONS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "jpg", "jpeg", "png", "gif", "bmp", "mp3", "mp4",
    "avi", "mov", "wav", "zip", "rar", "7z", "csv", "html",
];

/// Windows system programs that malware likes to imitate.
const SYSTEM_NAMES: &[&str] = &[
    "svchost.exe", "csrss.exe", "lsass.exe", "winlogon.exe", "services.exe", "smss.exe", "rundll32.exe",
    "explorer.exe", "dllhost.exe", "taskhostw.exe", "spoolsv.exe", "wininit.exe", "conhost.exe",
];

/// Why a file name looks suspicious, or `None` if it doesn't.
pub fn suspicious_reason(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let parts: Vec<&str> = name.split('.').collect();
    let extension = if parts.len() > 1 { *parts.last()? } else { "" };
    let is_executable = EXECUTABLE_EXTENSIONS.contains(&extension);
    let can_carry_code = is_executable || CODE_CARRYING_EXTENSIONS.contains(&extension);

    if can_carry_code {
        if let Some(word) = SUSPICIOUS_WORDS.iter().find(|w| name.contains(*w)) {
            return Some(format!("suspicious name ({})", word.trim_matches('_')));
        }
    }
    if is_executable && parts.len() > 2 {
        let fake = parts[parts.len() - 2];
        if DOCUMENT_EXTENSIONS.contains(&fake) {
            return Some(format!("double extension (.{fake}.{extension}): looks like a document but is a program"));
        }
    }
    if SYSTEM_NAMES.contains(&name.as_str()) {
        let in_windows = path
            .to_string_lossy()
            .to_lowercase()
            .replace('/', "\\")
            .contains(":\\windows\\");
        if !in_windows {
            return Some(format!("Windows system program name ({name}) outside the Windows folder"));
        }
    }
    None
}

fn security_table(lua: &Lua, policy: Arc<PathPolicy>, deadline: Instant) -> mlua::Result<Table> {
    let security = lua.create_table()?;
    security.set(
        "scan",
        lua.create_function(move |lua, (path, options): (String, Option<Table>)| {
            let dir = policy.resolve(&path).map_err(|e| err("security.scan", e))?;
            if !dir.is_dir() {
                return Err(err("security.scan", format!("directory not found: {path}")));
            }
            let recursive = match &options {
                Some(o) => o.get::<Option<bool>>("recursive")?.unwrap_or(true),
                None => true,
            };

            let mut hits: Vec<(PathBuf, String)> = Vec::new();
            let mut check = |file: &Path| {
                if let Some(reason) = suspicious_reason(file) {
                    hits.push((file.to_path_buf(), reason));
                }
                hits.len() < MAX_RESULTS
            };
            let complete = if recursive {
                walk_files(&dir, deadline, |file, _| check(file))
            } else {
                for entry in std::fs::read_dir(&dir).map_err(|e| err("security.scan", e))?.flatten() {
                    if entry.file_type().is_ok_and(|t| t.is_file()) {
                        check(&entry.path());
                    }
                }
                true
            };

            let results = lua.create_table()?;
            for (file, reason) in hits {
                let entry = lua.create_table()?;
                entry.set("path", path_string(&file))?;
                entry.set("reason", reason)?;
                results.push(entry)?;
            }
            Ok((results, complete))
        })?,
    )?;
    Ok(security)
}

#[cfg(test)]
mod tests {
    use super::suspicious_reason;
    use std::path::Path;

    #[test]
    fn flags_suspicious_names() {
        assert!(suspicious_reason(Path::new("C:/Users/me/Downloads/free-trojan-remover.exe")).is_some());
        assert!(suspicious_reason(Path::new("C:/Users/me/Downloads/invoice.pdf.exe")).unwrap().contains("double extension"));
        assert!(suspicious_reason(Path::new("C:/Users/me/AppData/svchost.exe")).unwrap().contains("system program"));
        assert!(suspicious_reason(Path::new("C:/Windows/System32/svchost.exe")).is_none());
        assert!(suspicious_reason(Path::new("C:/Users/me/Documents/report.pdf")).is_none());
        assert!(suspicious_reason(Path::new("C:/Users/me/setup.exe")).is_none());
        // Harmless file types are not flagged just for their name.
        assert!(suspicious_reason(Path::new("C:/Users/me/Pictures/virus-icon.png")).is_none());
        assert!(suspicious_reason(Path::new("C:/Temp/extension.payload.vsix")).is_none());
        assert!(suspicious_reason(Path::new("C:/Users/me/Downloads/keygen.zip")).is_some());
    }
}
