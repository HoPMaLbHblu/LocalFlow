//! Files and media: duplicates, pictures, photo dates, CSV tables and password-protected files.
//!
//! ```lua
//! fs.duplicates(folder, pattern)       -- groups of identical files: { { size, hash, files = {...} }, ... }
//! image.info(path)                     -- { width, height, format }
//! image.resize(source, destination, max_width, max_height)  -- keeps the shape; returns the new path
//! image.convert(source, destination)   -- the format comes from the new extension (.png, .jpg, .webp, ...)
//! image.taken(path)                    -- when a photo was taken (EXIF), as a timestamp, or nil
//! csv.read(path, { header = true })    -- rows; with a header, each row is a table by column name
//! csv.write(path, rows, { header = {...} })
//! crypto.encrypt(source, destination, password)
//! crypto.decrypt(source, destination, password)
//! crypto.password(length)              -- a random password
//! ```
//!
//! Like every other function, these only touch allowed folders, and a file that
//! would be replaced goes to the Recycle Bin first.

use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use mlua::{Lua, Table, Value};
use sha2::{Digest, Sha256};

use super::{
    files::walk_files,
    recycle,
    sandbox::{wildcard_match, PathPolicy},
};

fn err(function: &str, error: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(format!("{function}: {error}"))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Pictures bigger than this are refused, so a script can't run out of memory.
const MAX_IMAGE_PIXELS: u64 = 100_000_000;
/// Files up to this size can be encrypted (they are processed in memory).
const MAX_CRYPT_BYTES: u64 = 1024 * 1024 * 1024;
/// How many files `fs.duplicates` looks at before giving up.
const MAX_DUPLICATE_FILES: usize = 200_000;

/// Start of every file made by `crypto.encrypt`, so the wrong file is recognised.
const MAGIC: &[u8; 8] = b"LFCRYPT1";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;

pub fn register(lua: &Lua, fs: &Table, policy: Arc<PathPolicy>, deadline: Instant) -> mlua::Result<()> {
    register_duplicates(lua, fs, policy.clone(), deadline)?;
    let globals = lua.globals();
    globals.set("image", image_table(lua, policy.clone())?)?;
    globals.set("csv", csv_table(lua, policy.clone())?)?;
    globals.set("crypto", crypto_table(lua, policy)?)?;
    Ok(())
}

/// Resolve a destination, create its folder, and move an existing file to the Recycle Bin.
fn prepare_destination(policy: &PathPolicy, function: &str, path: &str) -> mlua::Result<PathBuf> {
    let resolved = policy.resolve(path).map_err(|e| err(function, e))?;
    if resolved.is_dir() {
        return Err(err(function, format!("{path} is a folder; give a file name")));
    }
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent).map_err(|e| err(function, e))?;
    }
    recycle::keep_old_version(&resolved).map_err(|e| err(function, e))?;
    Ok(resolved)
}

fn existing_file(policy: &PathPolicy, function: &str, path: &str) -> mlua::Result<PathBuf> {
    let resolved = policy.resolve(path).map_err(|e| err(function, e))?;
    if !resolved.is_file() {
        return Err(err(function, format!("file not found: {path}")));
    }
    Ok(resolved)
}

// ---- duplicates ---------------------------------------------------------------

fn file_hash(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Find files with identical contents. Files are first grouped by size, so only
/// files that could match are read.
pub fn find_duplicates(
    root: &Path,
    pattern: &str,
    deadline: Instant,
) -> (Vec<(u64, String, Vec<PathBuf>)>, bool) {
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut seen = 0usize;
    let mut complete = walk_files(root, deadline, |file, meta| {
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // Empty files are all "the same", which isn't useful.
        if meta.len() > 0 && wildcard_match(pattern, &name) {
            by_size.entry(meta.len()).or_default().push(file.to_path_buf());
            seen += 1;
        }
        seen < MAX_DUPLICATE_FILES
    });
    if seen >= MAX_DUPLICATE_FILES {
        complete = false;
    }

    let mut groups = Vec::new();
    let mut sizes: Vec<_> = by_size.into_iter().filter(|(_, files)| files.len() > 1).collect();
    // Biggest first: those free the most space.
    sizes.sort_by(|a, b| b.0.cmp(&a.0));
    'outer: for (size, files) in sizes {
        let mut by_hash: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for file in files {
            if Instant::now() >= deadline {
                complete = false;
                break 'outer;
            }
            if let Ok(hash) = file_hash(&file) {
                by_hash.entry(hash).or_default().push(file);
            }
        }
        let mut same: Vec<_> = by_hash.into_iter().filter(|(_, f)| f.len() > 1).collect();
        same.sort_by(|a, b| a.0.cmp(&b.0));
        for (hash, mut files) in same {
            files.sort();
            groups.push((size, hash, files));
        }
    }
    (groups, complete)
}

fn register_duplicates(lua: &Lua, fs: &Table, policy: Arc<PathPolicy>, deadline: Instant) -> mlua::Result<()> {
    fs.set(
        "duplicates",
        lua.create_function(move |lua, (path, pattern): (String, Option<String>)| {
            let dir = policy.resolve(&path).map_err(|e| err("fs.duplicates", e))?;
            if !dir.is_dir() {
                return Err(err("fs.duplicates", format!("directory not found: {path}")));
            }
            let (groups, complete) = find_duplicates(&dir, pattern.as_deref().unwrap_or("*"), deadline);
            let results = lua.create_table()?;
            for (size, hash, files) in groups {
                let group = lua.create_table()?;
                group.set("size", size)?;
                group.set("hash", hash)?;
                let list = lua.create_table()?;
                for file in files {
                    list.push(path_string(&file))?;
                }
                group.set("files", list)?;
                results.push(group)?;
            }
            Ok((results, complete))
        })?,
    )
}

// ---- pictures -------------------------------------------------------------------

fn open_image(path: &Path, function: &str) -> mlua::Result<image::DynamicImage> {
    let reader = image::ImageReader::open(path)
        .map_err(|e| err(function, e))?
        .with_guessed_format()
        .map_err(|e| err(function, e))?;
    let (width, height) = reader.into_dimensions().map_err(|e| err(function, format!("not a picture LocalFlow can read ({e})")))?;
    if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(err(function, format!("the picture is too big ({width} x {height})")));
    }
    image::ImageReader::open(path)
        .map_err(|e| err(function, e))?
        .with_guessed_format()
        .map_err(|e| err(function, e))?
        .decode()
        .map_err(|e| err(function, format!("not a picture LocalFlow can read ({e})")))
}

fn output_format(path: &Path, function: &str) -> mlua::Result<image::ImageFormat> {
    image::ImageFormat::from_path(path).map_err(|_| {
        err(function, "the new file needs a picture extension: .png, .jpg, .webp, .gif or .bmp")
    })
}

fn save_image(img: &image::DynamicImage, path: &Path, format: image::ImageFormat, function: &str) -> mlua::Result<()> {
    // JPEG has no transparency, so flatten first.
    let result = if format == image::ImageFormat::Jpeg {
        image::DynamicImage::ImageRgb8(img.to_rgb8()).save_with_format(path, format)
    } else {
        img.save_with_format(path, format)
    };
    result.map_err(|e| err(function, e))
}

fn format_name(format: image::ImageFormat) -> String {
    format.extensions_str().first().copied().unwrap_or("unknown").to_string()
}

/// EXIF "date taken" as seconds since 1970, read as local time.
pub fn photo_taken(path: &Path) -> Option<i64> {
    let file = File::open(path).ok()?;
    let exif = exif::Reader::new().read_from_container(&mut BufReader::new(file)).ok()?;
    let field = [exif::Tag::DateTimeOriginal, exif::Tag::DateTimeDigitized, exif::Tag::DateTime]
        .iter()
        .find_map(|tag| exif.get_field(*tag, exif::In::PRIMARY))?;
    let exif::Value::Ascii(ref parts) = field.value else { return None };
    let text = String::from_utf8_lossy(parts.first()?);
    let naive = chrono::NaiveDateTime::parse_from_str(text.trim(), "%Y:%m:%d %H:%M:%S").ok()?;
    use chrono::TimeZone;
    chrono::Local.from_local_datetime(&naive).earliest().map(|t| t.timestamp())
}

fn image_table(lua: &Lua, policy: Arc<PathPolicy>) -> mlua::Result<Table> {
    let image = lua.create_table()?;

    let p = policy.clone();
    image.set(
        "info",
        lua.create_function(move |lua, path: String| {
            let resolved = existing_file(&p, "image.info", &path)?;
            let reader = image::ImageReader::open(&resolved)
                .map_err(|e| err("image.info", e))?
                .with_guessed_format()
                .map_err(|e| err("image.info", e))?;
            let format = reader.format();
            let (width, height) = reader
                .into_dimensions()
                .map_err(|e| err("image.info", format!("not a picture LocalFlow can read ({e})")))?;
            let info = lua.create_table()?;
            info.set("width", width)?;
            info.set("height", height)?;
            info.set("format", format.map(format_name))?;
            Ok(info)
        })?,
    )?;

    let p = policy.clone();
    image.set(
        "resize",
        lua.create_function(
            move |_, (source, destination, max_width, max_height): (String, String, u32, Option<u32>)| {
                if max_width == 0 || max_height == Some(0) {
                    return Err(err("image.resize", "sizes must be at least 1 pixel"));
                }
                let from = existing_file(&p, "image.resize", &source)?;
                let img = open_image(&from, "image.resize")?;
                let to_check = p.resolve(&destination).map_err(|e| err("image.resize", e))?;
                let format = output_format(&to_check, "image.resize")?;
                let max_height = max_height.unwrap_or(max_width);
                // Only ever shrink; small pictures stay as they are.
                let resized = if img.width() > max_width || img.height() > max_height {
                    img.resize(max_width, max_height, image::imageops::FilterType::Lanczos3)
                } else {
                    img
                };
                let to = prepare_destination(&p, "image.resize", &destination)?;
                save_image(&resized, &to, format, "image.resize")?;
                Ok(path_string(&to))
            },
        )?,
    )?;

    let p = policy.clone();
    image.set(
        "convert",
        lua.create_function(move |_, (source, destination): (String, String)| {
            let from = existing_file(&p, "image.convert", &source)?;
            let img = open_image(&from, "image.convert")?;
            let to_check = p.resolve(&destination).map_err(|e| err("image.convert", e))?;
            let format = output_format(&to_check, "image.convert")?;
            let to = prepare_destination(&p, "image.convert", &destination)?;
            save_image(&img, &to, format, "image.convert")?;
            Ok(path_string(&to))
        })?,
    )?;

    let p = policy;
    image.set(
        "taken",
        lua.create_function(move |_, path: String| {
            let resolved = existing_file(&p, "image.taken", &path)?;
            Ok(photo_taken(&resolved))
        })?,
    )?;

    Ok(image)
}

// ---- CSV ----------------------------------------------------------------------

fn cell_text(value: Value) -> mlua::Result<String> {
    Ok(match value {
        Value::Nil => String::new(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.to_str()?.to_string(),
        other => return Err(err("csv.write", format!("cells must be text, numbers or booleans, not {}", other.type_name()))),
    })
}

fn option<T: mlua::FromLua>(options: &Option<Table>, key: &str) -> mlua::Result<Option<T>> {
    match options {
        Some(t) => t.get(key),
        None => Ok(None),
    }
}

fn delimiter(options: &Option<Table>, function: &str) -> mlua::Result<u8> {
    match option::<String>(options, "separator")? {
        None => Ok(b','),
        Some(s) if s.len() == 1 => Ok(s.as_bytes()[0]),
        Some(s) if s == "\\t" || s == "tab" => Ok(b'\t'),
        Some(_) => Err(err(function, "separator must be one character, like \",\" or \";\"")),
    }
}

fn csv_table(lua: &Lua, policy: Arc<PathPolicy>) -> mlua::Result<Table> {
    let csv = lua.create_table()?;

    let p = policy.clone();
    csv.set(
        "read",
        lua.create_function(move |lua, (path, options): (String, Option<Table>)| {
            let resolved = existing_file(&p, "csv.read", &path)?;
            let header = option::<bool>(&options, "header")?.unwrap_or(true);
            let mut reader = csv::ReaderBuilder::new()
                .has_headers(header)
                .flexible(true)
                .delimiter(delimiter(&options, "csv.read")?)
                .from_path(&resolved)
                .map_err(|e| err("csv.read", e))?;
            let names: Vec<String> = if header {
                reader.headers().map_err(|e| err("csv.read", e))?.iter().map(str::to_string).collect()
            } else {
                Vec::new()
            };
            let rows = lua.create_table()?;
            for record in reader.records() {
                let record = record.map_err(|e| err("csv.read", e))?;
                let row = lua.create_table()?;
                for (i, cell) in record.iter().enumerate() {
                    match names.get(i) {
                        Some(name) if header => row.set(name.as_str(), cell)?,
                        _ => row.set(i + 1, cell)?,
                    }
                }
                rows.push(row)?;
            }
            Ok(rows)
        })?,
    )?;

    let p = policy;
    csv.set(
        "write",
        lua.create_function(move |_, (path, rows, options): (String, Table, Option<Table>)| {
            let header: Option<Vec<String>> = option::<Vec<String>>(&options, "header")?;
            let separator = delimiter(&options, "csv.write")?;
            let mut out: Vec<u8> = Vec::new();
            {
                let mut writer = csv::WriterBuilder::new().delimiter(separator).flexible(true).from_writer(&mut out);
                if let Some(names) = &header {
                    writer.write_record(names).map_err(|e| err("csv.write", e))?;
                }
                for row in rows.sequence_values::<Table>() {
                    let row = row.map_err(|_| err("csv.write", "rows must be tables"))?;
                    let cells: Vec<String> = match &header {
                        // With a header, rows may be keyed by column name.
                        Some(names) if row.raw_len() == 0 => {
                            names.iter().map(|n| cell_text(row.get(n.as_str())?)).collect::<mlua::Result<_>>()?
                        }
                        _ => row.sequence_values::<Value>().map(|v| cell_text(v?)).collect::<mlua::Result<_>>()?,
                    };
                    writer.write_record(&cells).map_err(|e| err("csv.write", e))?;
                }
                writer.flush().map_err(|e| err("csv.write", e))?;
            }
            let to = prepare_destination(&p, "csv.write", &path)?;
            std::fs::write(&to, out).map_err(|e| err("csv.write", e))?;
            Ok(path_string(&to))
        })?,
    )?;

    Ok(csv)
}

// ---- password-protected files ---------------------------------------------------

fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let mut key = [0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| e.to_string())?;
    Ok(key)
}

/// `MAGIC | salt | nonce | ciphertext` (XChaCha20-Poly1305, key from Argon2id).
pub fn encrypt_bytes(plain: &[u8], password: &str) -> Result<Vec<u8>, String> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut salt).map_err(|e| e.to_string())?;
    getrandom::getrandom(&mut nonce).map_err(|e| e.to_string())?;
    let key = derive_key(password, &salt)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let sealed = cipher.encrypt(XNonce::from_slice(&nonce), plain).map_err(|_| "encryption failed".to_string())?;
    let mut out = Vec::with_capacity(MAGIC.len() + SALT_LEN + NONCE_LEN + sealed.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok(out)
}

pub fn decrypt_bytes(data: &[u8], password: &str) -> Result<Vec<u8>, String> {
    let header = MAGIC.len() + SALT_LEN + NONCE_LEN;
    if data.len() < header || &data[..MAGIC.len()] != MAGIC {
        return Err("this file was not made by crypto.encrypt".into());
    }
    let salt = &data[MAGIC.len()..MAGIC.len() + SALT_LEN];
    let nonce = &data[MAGIC.len() + SALT_LEN..header];
    let key = derive_key(password, salt)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    cipher
        .decrypt(XNonce::from_slice(nonce), &data[header..])
        .map_err(|_| "wrong password, or the file is damaged".into())
}

fn read_limited(path: &Path, function: &str) -> mlua::Result<Vec<u8>> {
    let size = std::fs::metadata(path).map_err(|e| err(function, e))?.len();
    if size > MAX_CRYPT_BYTES {
        return Err(err(function, "files over 1 GB can't be encrypted; zip them into smaller parts first"));
    }
    std::fs::read(path).map_err(|e| err(function, e))
}

fn check_password(password: &str, function: &str) -> mlua::Result<()> {
    if password.chars().count() < 8 {
        return Err(err(function, "use a password of at least 8 characters"));
    }
    Ok(())
}

fn crypto_table(lua: &Lua, policy: Arc<PathPolicy>) -> mlua::Result<Table> {
    let crypto = lua.create_table()?;

    let p = policy.clone();
    crypto.set(
        "encrypt",
        lua.create_function(move |_, (source, destination, password): (String, String, String)| {
            check_password(&password, "crypto.encrypt")?;
            let from = existing_file(&p, "crypto.encrypt", &source)?;
            let sealed = encrypt_bytes(&read_limited(&from, "crypto.encrypt")?, &password)
                .map_err(|e| err("crypto.encrypt", e))?;
            // Never replace the original: that would lose it if the password is forgotten.
            let to = p.resolve(&destination).map_err(|e| err("crypto.encrypt", e))?;
            if to == from {
                return Err(err("crypto.encrypt", "save the locked copy under a new name, e.g. \"file.txt.locked\""));
            }
            let to = prepare_destination(&p, "crypto.encrypt", &destination)?;
            std::fs::write(&to, sealed).map_err(|e| err("crypto.encrypt", e))?;
            Ok(path_string(&to))
        })?,
    )?;

    let p = policy;
    crypto.set(
        "decrypt",
        lua.create_function(move |_, (source, destination, password): (String, String, String)| {
            let from = existing_file(&p, "crypto.decrypt", &source)?;
            let plain = decrypt_bytes(&read_limited(&from, "crypto.decrypt")?, &password)
                .map_err(|e| err("crypto.decrypt", e))?;
            let to = prepare_destination(&p, "crypto.decrypt", &destination)?;
            std::fs::write(&to, plain).map_err(|e| err("crypto.decrypt", e))?;
            Ok(path_string(&to))
        })?,
    )?;

    crypto.set(
        "password",
        lua.create_function(|_, length: Option<usize>| {
            const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789-_!?";
            let length = length.unwrap_or(16).clamp(8, 128);
            let mut bytes = vec![0u8; length];
            getrandom::getrandom(&mut bytes).map_err(|e| err("crypto.password", e))?;
            Ok(bytes.iter().map(|b| CHARS[*b as usize % CHARS.len()] as char).collect::<String>())
        })?,
    )?;

    Ok(crypto)
}
