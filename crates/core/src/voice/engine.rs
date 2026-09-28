//! Speech engines and their models: catalogue, download, verification, recognition, VAD.
//!
//! The engine is sherpa-onnx (small streaming zipformer models, one per language) plus Silero VAD.
//! Everything runs on this PC; audio never leaves it. Models are downloaded only when
//! `download_model` is called (never automatically) into `<data dir>/voice/models/<model id>/`.
//!
//! The catalogue, model status, download and archive handling are plain Rust and always compiled
//! (and tested without the `voice-engine` feature); only recognition and voice activity detection
//! need the feature. Without it `create_recognizer` / `create_segmenter` return
//! `Err("this build has no voice engine")`.

#[cfg(feature = "voice-engine")]
use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, Write};
#[cfg(feature = "voice-engine")]
use std::path::Component;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::Recognizer;

pub use super::audio::NO_ENGINE;

/// The error text of a download stopped by the user (`cancel` flag).
pub const CANCELLED: &str = "cancelled";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Download size of the model, 0 if nothing is downloaded.
    pub download_bytes: u64,
    pub languages: Vec<String>,
    /// Audio never leaves this PC.
    pub local: bool,
    pub license: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelStatus {
    pub engine: String,
    pub installed: bool,
    pub size_bytes: u64,
}

// ---- catalogue ---------------------------------------------------------------------------------

/// How a catalogue entry is delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packaging {
    /// A `.tar.bz2` archive with one top-level folder.
    TarBz2,
    /// One plain file (the file name is the last part of the URL).
    SingleFile,
}

/// One downloadable model with everything needed to verify it.
#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub id: String,
    pub name: String,
    pub languages: Vec<String>,
    pub url: String,
    /// Exact size of the downloaded file in bytes.
    pub bytes: u64,
    /// SHA-256 of the downloaded file, lower-case hex.
    pub sha256: String,
    pub packaging: Packaging,
    /// Files that must exist after installing (relative to the model folder).
    pub required: Vec<String>,
    /// Upper bound for the unpacked size (guards against archive bombs).
    pub max_extracted_bytes: u64,
    pub license: String,
    /// Attribution text for the About screen.
    pub attribution: String,
}

const BASE_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/";
const VAD_ID: &str = "silero-vad";
const VAD_FILE: &str = "silero_vad.onnx";

const LOCAL_NOTE: &str = "Audio is processed on this PC and never leaves it; only the model file is downloaded (from github.com).";

fn kroko_spec(lang: &str, language_name: &str, file: &str, bytes: u64, sha: &str) -> ModelSpec {
    ModelSpec {
        id: format!("sherpa-{lang}"),
        name: format!("{language_name} (Kroko, streaming)"),
        languages: vec![lang.into()],
        url: format!("{BASE_URL}{file}"),
        bytes,
        sha256: sha.into(),
        packaging: Packaging::TarBz2,
        required: ["encoder.onnx", "decoder.onnx", "joiner.onnx", "tokens.txt"].map(String::from).to_vec(),
        max_extracted_bytes: 160 * 1024 * 1024,
        license: "CC-BY-SA (Kroko ASR, Banafo)".into(),
        attribution: "Speech model: Kroko ASR by Banafo (https://huggingface.co/Banafo/Kroko-ASR), licensed CC-BY-SA, \
                      used unmodified through sherpa-onnx (Apache-2.0)."
            .into(),
    }
}

/// The speech recognition models (not the VAD model).
pub fn catalogue() -> Vec<ModelSpec> {
    vec![
        kroko_spec(
            "en",
            "English",
            "sherpa-onnx-streaming-zipformer-en-kroko-2025-08-06.tar.bz2",
            57_267_600,
            "c8676e5ff9ac2a85296e53ee0fd4d5fb1db6770e7a7647166eeafe349ade6834",
        ),
        kroko_spec(
            "de",
            "German",
            "sherpa-onnx-streaming-zipformer-de-kroko-2025-08-06.tar.bz2",
            57_565_698,
            "9e27b783c20e67b0d0f13a258c1861fce199917c969d9176a438bee38df64962",
        ),
        ModelSpec {
            id: "sherpa-ru".into(),
            name: "Russian (Vosk small, streaming)".into(),
            languages: vec!["ru".into()],
            url: format!("{BASE_URL}sherpa-onnx-streaming-zipformer-small-ru-vosk-int8-2025-08-16.tar.bz2"),
            bytes: 24_110_855,
            sha256: "6ba68a01ff3c5445aaf2d61e9b97b026f1149dcc9049d11af3f44f55176341d8".into(),
            packaging: Packaging::TarBz2,
            required: ["encoder.int8.onnx", "decoder.onnx", "joiner.int8.onnx", "tokens.txt", "bpe.model"]
                .map(String::from)
                .to_vec(),
            max_extracted_bytes: 80 * 1024 * 1024,
            license: "Apache-2.0 (Vosk small streaming model, Alpha Cephei)".into(),
            attribution: "Speech model: vosk-model-small-streaming-ru by Alpha Cephei \
                          (https://huggingface.co/alphacep/vosk-model-small-streaming-ru), Apache-2.0, converted for sherpa-onnx."
                .into(),
        },
    ]
}

/// The voice activity detection model, installed together with the first speech model.
pub fn vad_spec() -> ModelSpec {
    ModelSpec {
        id: VAD_ID.into(),
        name: "Silero VAD".into(),
        languages: Vec::new(),
        url: format!("{BASE_URL}{VAD_FILE}"),
        bytes: 643_854,
        sha256: "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6".into(),
        packaging: Packaging::SingleFile,
        required: vec![VAD_FILE.into()],
        max_extracted_bytes: 4 * 1024 * 1024,
        license: "MIT (Silero)".into(),
        attribution: "Voice activity detection: Silero VAD (https://github.com/snakers4/silero-vad), MIT licence.".into(),
    }
}

fn find_spec(model_id: &str) -> Option<ModelSpec> {
    catalogue().into_iter().find(|s| s.id == model_id)
}

/// All licence and attribution notices for the About screen.
pub fn attribution_notices() -> String {
    let mut out = vec![
        "Speech recognition runtime: sherpa-onnx (https://github.com/k2-fsa/sherpa-onnx), Apache-2.0.".to_string(),
        vad_spec().attribution,
    ];
    out.extend(catalogue().into_iter().map(|s| s.attribution));
    out.push(LOCAL_NOTE.to_string());
    out.join("\n")
}

#[cfg(not(feature = "voice-engine"))]
pub fn available_engines() -> Vec<EngineInfo> {
    Vec::new()
}

#[cfg(feature = "voice-engine")]
pub fn available_engines() -> Vec<EngineInfo> {
    let vad = vad_spec();
    catalogue()
        .into_iter()
        .map(|s| EngineInfo {
            description: format!(
                "Offline speech recognition for {} ({} MB download, plus {} KB voice detector). Licence: {}. {} {} {}",
                s.languages.join(", "),
                s.bytes / 1_000_000,
                vad.bytes / 1000,
                s.license,
                s.attribution,
                vad.attribution,
                LOCAL_NOTE
            ),
            download_bytes: s.bytes + vad.bytes,
            languages: s.languages.clone(),
            local: true,
            license: format!("{}; {}", s.license, vad.license),
            id: s.id,
            name: s.name,
        })
        .collect()
}

// ---- where models live ---------------------------------------------------------------------------

/// `<data dir>/voice/models`.
pub fn models_dir() -> PathBuf {
    crate::appdata::dir().join("voice").join("models")
}

const MANIFEST: &str = "manifest.json";

#[derive(Serialize, Deserialize)]
struct Manifest {
    id: String,
    sha256: String,
    bytes: u64,
    files: Vec<(String, u64)>,
}

/// Is `spec` installed and intact in `dir`? Returns the unpacked size. Checks the manifest written
/// at install time (which records the verified archive checksum) and every file's presence and size.
pub fn installed_size_in(dir: &Path, spec: &ModelSpec) -> Option<u64> {
    let folder = dir.join(&spec.id);
    let manifest: Manifest = serde_json::from_slice(&fs::read(folder.join(MANIFEST)).ok()?).ok()?;
    if manifest.id != spec.id || manifest.sha256 != spec.sha256 || manifest.bytes != spec.bytes {
        return None;
    }
    for required in &spec.required {
        if !manifest.files.iter().any(|(n, _)| n == required) {
            return None;
        }
    }
    let mut total = 0;
    for (name, size) in &manifest.files {
        let meta = fs::metadata(folder.join(name)).ok()?;
        if !meta.is_file() || meta.len() != *size {
            return None;
        }
        total += size;
    }
    Some(total)
}

/// Status of a speech model: installed only when the language model AND the VAD model are present
/// and verified.
pub fn model_status_in(dir: &Path, spec: &ModelSpec, vad: &ModelSpec) -> ModelStatus {
    match (installed_size_in(dir, spec), installed_size_in(dir, vad)) {
        (Some(a), Some(b)) => ModelStatus { engine: spec.id.clone(), installed: true, size_bytes: a + b },
        _ => ModelStatus { engine: spec.id.clone(), installed: false, size_bytes: 0 },
    }
}

pub fn model_status(engine: &str) -> ModelStatus {
    match find_spec(engine) {
        Some(spec) => model_status_in(&models_dir(), &spec, &vad_spec()),
        None => ModelStatus { engine: engine.into(), installed: false, size_bytes: 0 },
    }
}

// ---- fetching ------------------------------------------------------------------------------------

/// An open download.
pub struct Download {
    pub reader: Box<dyn Read + Send>,
    /// Length announced by the server, when known.
    pub length: Option<u64>,
}

/// Where model files come from. The real one is HTTPS only; tests inject their own.
pub trait Fetcher: Send + Sync {
    fn open(&self, url: &str) -> Result<Download, String>;
}

/// HTTPS downloads with `User-Agent: LocalFlow/<version>`. Plain HTTP (also in redirects) is refused.
pub struct HttpsFetcher;

impl Fetcher for HttpsFetcher {
    fn open(&self, url: &str) -> Result<Download, String> {
        if !url.starts_with("https://") {
            return Err("model downloads must use HTTPS".into());
        }
        let agent = ureq::AgentBuilder::new()
            .https_only(true)
            .timeout_connect(std::time::Duration::from_secs(15))
            .timeout_read(std::time::Duration::from_secs(5))
            .user_agent(&format!("LocalFlow/{}", env!("CARGO_PKG_VERSION")))
            .build();
        let response = agent.get(url).call().map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("the download server answered {code}"),
            other => format!("cannot reach the download server: {other}"),
        })?;
        let length = response.header("Content-Length").and_then(|v| v.trim().parse().ok());
        Ok(Download { reader: Box::new(response.into_reader()), length })
    }
}

// ---- installing ------------------------------------------------------------------------------------

/// Removes temporary files and folders on drop unless disarmed.
struct Cleanup(Vec<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for p in &self.0 {
            if p.is_dir() {
                let _ = fs::remove_dir_all(p);
            } else {
                let _ = fs::remove_file(p);
            }
        }
    }
}

fn unique(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!(".tmp-{tag}-{}-{nanos}", std::process::id())
}

/// Remove leftovers of interrupted downloads (never touches installed models).
pub fn clean_stale_temp_files(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(".tmp-") || name.starts_with(".old-") {
                let p = e.path();
                if p.is_dir() {
                    let _ = fs::remove_dir_all(&p);
                } else {
                    let _ = fs::remove_file(&p);
                }
            }
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Stream a download into `part`, hashing as it goes. `report(bytes_so_far)` is called regularly.
fn fetch_to_file(
    fetcher: &dyn Fetcher,
    spec: &ModelSpec,
    part: &Path,
    report: &mut dyn FnMut(u64),
    cancel: &AtomicBool,
) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }
    let mut download = fetcher.open(&spec.url)?;
    if let Some(len) = download.length {
        if len != spec.bytes {
            return Err(format!("the server announced {len} bytes for {}, expected {}", spec.name, spec.bytes));
        }
    }
    let mut file = fs::File::create(part).map_err(|e| format!("cannot write the model file: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done: u64 = 0;
    let mut last_report = 0u64;
    let mut stalls = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        match download.reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                stalls = 0;
                done += n as u64;
                if done > spec.bytes {
                    return Err(format!("the download of {} is larger than expected", spec.name));
                }
                hasher.update(&buf[..n]);
                file.write_all(&buf[..n]).map_err(|e| format!("cannot write the model file: {e}"))?;
                if done - last_report >= 256 * 1024 {
                    last_report = done;
                    report(done);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {
                stalls += 1;
                if stalls > 6 {
                    return Err("the download stalled (no data for 30 seconds)".into());
                }
            }
            Err(e) => return Err(format!("the download was interrupted: {e}")),
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    report(done);
    if done != spec.bytes {
        return Err(format!("the download of {} is incomplete ({done} of {} bytes)", spec.name, spec.bytes));
    }
    let digest = hex(&hasher.finalize());
    if digest != spec.sha256 {
        return Err(format!(
            "the downloaded file for {} failed the checksum test (it was corrupted or altered) and was not installed",
            spec.name
        ));
    }
    Ok(())
}

#[cfg(feature = "voice-engine")]
/// Validate an archive entry path and drop its top-level folder. `None` = skip the entry.
fn clean_entry_path(path: &Path) -> Result<Option<PathBuf>, String> {
    let mut parts: Vec<&std::ffi::OsStr> = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => {
                let s = p.to_string_lossy();
                if s.contains(':') || s.contains('\\') || s.contains('\0') {
                    return Err(format!("unsafe path in the archive: {}", path.display()));
                }
                parts.push(p);
            }
            Component::CurDir => {}
            _ => return Err(format!("unsafe path in the archive: {}", path.display())),
        }
    }
    if parts.len() < 2 {
        // The top-level folder itself (or a stray file outside it).
        return Ok(None);
    }
    if parts[1] == "test_wavs" {
        return Ok(None);
    }
    Ok(Some(parts[1..].iter().collect()))
}

/// Safely unpack a `.tar.bz2` model archive into `dest`: only regular files and folders, no
/// absolute paths, no `..`, no links, the top-level folder is stripped, `test_wavs` is skipped and
/// at most `max_bytes` are written. Returns the list of (relative file, size).
#[cfg(not(feature = "voice-engine"))]
pub fn extract_tar_bz2(_archive: &Path, _dest: &Path, _max_bytes: u64, _cancel: &AtomicBool) -> Result<Vec<(String, u64)>, String> {
    Err(NO_ENGINE.into())
}

#[cfg(feature = "voice-engine")]
pub fn extract_tar_bz2(
    archive: &Path,
    dest: &Path,
    max_bytes: u64,
    cancel: &AtomicBool,
) -> Result<Vec<(String, u64)>, String> {
    let file = fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(io::BufReader::new(file)));
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let mut written_total: u64 = 0;
    let mut files: Vec<(String, u64)> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let entries = tar.entries().map_err(|e| format!("the archive is damaged: {e}"))?;
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        let mut entry = entry.map_err(|e| format!("the archive is damaged: {e}"))?;
        let kind = entry.header().entry_type();
        let path = entry.path().map_err(|e| format!("the archive is damaged: {e}"))?.into_owned();
        // Validate the path of every entry, whatever its type.
        let rel = clean_entry_path(&path)?;
        if kind.is_pax_global_extensions() || kind.is_pax_local_extensions() {
            continue;
        }
        if !(kind.is_file() || kind.is_dir()) {
            return Err(format!("the archive contains a link or special file ({}), which is not allowed", path.display()));
        }
        let Some(rel) = rel else { continue };
        let target = dest.join(&rel);
        if kind.is_dir() {
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        if !seen.insert(rel.clone()) {
            return Err(format!("the archive contains {} twice", rel.display()));
        }
        if written_total.saturating_add(entry.size()) > max_bytes {
            return Err("the archive unpacks to more than the allowed size".into());
        }
        let mut out = fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 64 * 1024];
        let mut size = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(CANCELLED.into());
            }
            let n = entry.read(&mut buf).map_err(|e| format!("the archive is damaged: {e}"))?;
            if n == 0 {
                break;
            }
            size += n as u64;
            written_total += n as u64;
            if written_total > max_bytes {
                return Err("the archive unpacks to more than the allowed size".into());
            }
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        }
        let name = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join("/");
        files.push((name, size));
    }
    Ok(files)
}

/// Download, verify and install one catalogue entry into `dir/<spec.id>`.
/// `progress(done, total)` receives `base + bytes of this entry` and `total`. Nothing is left behind
/// (temp files, half-unpacked folders) on failure or cancel, and an existing installation is only
/// replaced after the new one is complete.
pub fn install_with(
    spec: &ModelSpec,
    dir: &Path,
    fetcher: &dyn Fetcher,
    base: u64,
    total: u64,
    progress: &dyn Fn(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("cannot create the models folder: {e}"))?;
    let tag = unique(&spec.id);
    let part = dir.join(format!("{tag}.part"));
    let staging = dir.join(format!("{tag}.dir"));
    let _cleanup = Cleanup(vec![part.clone(), staging.clone()]);

    fetch_to_file(fetcher, spec, &part, &mut |done| progress(base + done, total), cancel)?;

    let files = match spec.packaging {
        Packaging::TarBz2 => extract_tar_bz2(&part, &staging, spec.max_extracted_bytes, cancel)?,
        Packaging::SingleFile => {
            let name = spec.required.first().cloned().ok_or("model entry without a file name")?;
            fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
            fs::rename(&part, staging.join(&name)).map_err(|e| e.to_string())?;
            vec![(name, spec.bytes)]
        }
    };
    for required in &spec.required {
        if !files.iter().any(|(n, _)| n == required) {
            return Err(format!("the archive for {} lacks the file {required}", spec.name));
        }
    }
    let manifest = Manifest { id: spec.id.clone(), sha256: spec.sha256.clone(), bytes: spec.bytes, files };
    fs::write(staging.join(MANIFEST), serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if cancel.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }

    // Swap in with renames; keep the old copy until the new one is in place.
    let final_dir = dir.join(&spec.id);
    let old = dir.join(format!("{tag}.old"));
    let had_old = final_dir.exists();
    if had_old {
        fs::rename(&final_dir, &old).map_err(|e| format!("cannot replace the installed model: {e}"))?;
    }
    if let Err(e) = fs::rename(&staging, &final_dir) {
        if had_old {
            let _ = fs::rename(&old, &final_dir);
        }
        return Err(format!("cannot install the model: {e}"));
    }
    if had_old {
        let _ = fs::remove_dir_all(&old);
    }
    progress(base + spec.bytes, total);
    Ok(())
}

/// Install a speech model (and the VAD model when missing) from `fetcher` into `dir`.
pub fn download_into(
    dir: &Path,
    spec: &ModelSpec,
    vad: &ModelSpec,
    fetcher: &dyn Fetcher,
    progress: &dyn Fn(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("cannot create the models folder: {e}"))?;
    clean_stale_temp_files(dir);
    let mut todo: Vec<&ModelSpec> = Vec::new();
    if installed_size_in(dir, vad).is_none() {
        todo.push(vad);
    }
    if installed_size_in(dir, spec).is_none() {
        todo.push(spec);
    }
    let total: u64 = todo.iter().map(|s| s.bytes).sum();
    let mut base = 0;
    for s in todo {
        install_with(s, dir, fetcher, base, total, progress, cancel)?;
        base += s.bytes;
    }
    progress(total, total);
    Ok(())
}

/// Download and verify (checksum) the model. `progress(done, total)`; stops early if `cancel` is set
/// (error text `CANCELLED`). Only ever called on the user's request.
pub fn download_model(engine: &str, progress: &dyn Fn(u64, u64), cancel: &AtomicBool) -> Result<(), String> {
    if !cfg!(feature = "voice-engine") {
        return Err(NO_ENGINE.into());
    }
    let spec = find_spec(engine).ok_or_else(|| format!("unknown speech model: {engine}"))?;
    download_into(&models_dir(), &spec, &vad_spec(), &HttpsFetcher, progress, cancel)
}

/// Remove a model (and the VAD model when no speech model is left).
pub fn remove_model(engine: &str) -> Result<(), String> {
    let spec = find_spec(engine).ok_or_else(|| format!("unknown speech model: {engine}"))?;
    remove_from(&models_dir(), &spec)
}

pub fn remove_from(dir: &Path, spec: &ModelSpec) -> Result<(), String> {
    let folder = dir.join(&spec.id);
    if folder.exists() {
        fs::remove_dir_all(&folder).map_err(|e| format!("cannot remove the model: {e}"))?;
    }
    let others = catalogue().iter().any(|s| s.id != spec.id && dir.join(&s.id).exists());
    if !others {
        let vad = dir.join(VAD_ID);
        if vad.exists() {
            fs::remove_dir_all(&vad).map_err(|e| format!("cannot remove the model: {e}"))?;
        }
    }
    Ok(())
}

/// The model for a recognition language ("en", "ru", "de"), e.g. "sherpa-en". `engine` arguments of
/// the functions above are MODEL ids: one per language.
pub fn model_for_language(language: &str) -> Option<String> {
    match language {
        "en" | "ru" | "de" => Some(format!("sherpa-{language}")),
        _ => None,
    }
}

// ---- recognition and voice activity detection ------------------------------------------------------

#[cfg(feature = "voice-engine")]
pub use real::{create_recognizer, create_segmenter, create_segmenter_with, create_recognizer_in, SegmenterSettings};

#[cfg(not(feature = "voice-engine"))]
pub fn create_recognizer(_engine: &str, _language: &str) -> Result<Box<dyn Recognizer>, String> {
    Err(NO_ENGINE.into())
}

/// Voice activity detection (cuts the audio into utterances). Needs the VAD model.
#[cfg(not(feature = "voice-engine"))]
pub fn create_segmenter() -> Result<Box<dyn super::Segmenter>, String> {
    Err(NO_ENGINE.into())
}

#[cfg(feature = "voice-engine")]
mod real {
    use std::collections::VecDeque;

    use sherpa_onnx::{
        OnlineRecognizer, OnlineRecognizerConfig, SileroVadModelConfig, VadModelConfig, VoiceActivityDetector,
    };

    use super::*;
    use crate::voice::{Recognized, Segmented, Segmenter};

    const RATE: usize = 16_000;
    /// Silence added before and after an utterance. The streaming model has a long right context:
    /// with less than about 1.2 s of trailing silence it can drop the last word ("dark mode on" ->
    /// "dark mode"), measured with exact-zero padding.
    const LEAD_SILENCE: usize = RATE / 5;
    const TAIL_SILENCE: usize = RATE * 6 / 5;

    // ---- recognizer ----

    struct SherpaRecognizer {
        recognizer: OnlineRecognizer,
        language: String,
        name: String,
    }

    impl Recognizer for SherpaRecognizer {
        /// Decodes one whole utterance. The model is fixed per language; `language` is ignored.
        fn transcribe(&mut self, audio: &[f32], _language: &str) -> Result<Recognized, String> {
            let mut text = String::new();
            if !audio.is_empty() {
                let mut padded = Vec::with_capacity(audio.len() + LEAD_SILENCE + TAIL_SILENCE);
                padded.resize(LEAD_SILENCE, 0.0f32);
                padded.extend(audio.iter().map(|s| if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 }));
                padded.resize(padded.len() + TAIL_SILENCE, 0.0);
                let stream = self.recognizer.create_stream();
                stream.accept_waveform(RATE as i32, &padded);
                stream.input_finished();
                while self.recognizer.is_ready(&stream) {
                    self.recognizer.decode(&stream);
                }
                text = self.recognizer.get_result(&stream).map(|r| r.text).unwrap_or_default();
            }
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
            // The transducer gives no trustworthy confidence, and phrase biasing (hotwords) is
            // not used: matching against the command list is done by the voice layer.
            Ok(Recognized { text, confidence: None, language: Some(self.language.clone()) })
        }

        fn name(&self) -> String {
            self.name.clone()
        }
    }

    fn pick<'a>(spec: &'a ModelSpec, needle: &str) -> Result<&'a str, String> {
        spec.required.iter().find(|f| f.contains(needle)).map(String::as_str).ok_or_else(|| format!("{} has no {needle} file", spec.id))
    }

    /// Load a model installed in `dir`. Slow (a few seconds): call it off the UI thread.
    pub fn create_recognizer_in(dir: &Path, spec: &ModelSpec, vad: &ModelSpec) -> Result<Box<dyn Recognizer>, String> {
        if !model_status_in(dir, spec, vad).installed {
            return Err(format!("The speech model \"{}\" is not downloaded yet.", spec.name));
        }
        let folder = dir.join(&spec.id);
        let path = |f: &str| Some(folder.join(f).to_string_lossy().to_string());
        let mut cfg = OnlineRecognizerConfig::default();
        cfg.model_config.transducer.encoder = path(pick(spec, "encoder")?);
        cfg.model_config.transducer.decoder = path(pick(spec, "decoder")?);
        cfg.model_config.transducer.joiner = path(pick(spec, "joiner")?);
        cfg.model_config.tokens = path("tokens.txt");
        if spec.required.iter().any(|f| f == "bpe.model") {
            cfg.model_config.modeling_unit = Some("bpe".into());
            cfg.model_config.bpe_vocab = path("bpe.model");
        }
        cfg.model_config.num_threads = 2;
        cfg.enable_endpoint = false;
        let recognizer = OnlineRecognizer::create(&cfg)
            .ok_or_else(|| format!("The speech model \"{}\" could not be loaded. Try downloading it again.", spec.name))?;
        Ok(Box::new(SherpaRecognizer {
            recognizer,
            language: spec.languages.first().cloned().unwrap_or_default(),
            name: format!("sherpa-onnx: {}", spec.name),
        }))
    }

    /// A recognizer for a model id such as "sherpa-en". `language` is only checked for being known.
    pub fn create_recognizer(engine: &str, _language: &str) -> Result<Box<dyn Recognizer>, String> {
        let spec = find_spec(engine).ok_or_else(|| format!("unknown speech model: {engine}"))?;
        create_recognizer_in(&models_dir(), &spec, &vad_spec())
    }

    // ---- segmenter ----

    #[derive(Debug, Clone, Copy)]
    pub struct SegmenterSettings {
        /// Speech probability above which audio counts as speech.
        pub threshold: f32,
        /// Pause that ends an utterance.
        pub min_silence: f32,
        /// Shorter sounds are ignored.
        pub min_speech: f32,
        /// Longer utterances are cut here.
        pub max_speech: f32,
        /// Audio kept before the detected start so the first word is not clipped.
        pub pre_roll: f32,
        /// Audio kept after the detected end.
        pub post_roll: f32,
    }

    impl Default for SegmenterSettings {
        fn default() -> Self {
            SegmenterSettings { threshold: 0.5, min_silence: 0.6, min_speech: 0.2, max_speech: 15.0, pre_roll: 0.4, post_roll: 0.3 }
        }
    }

    struct SileroSegmenter {
        model: PathBuf,
        settings: SegmenterSettings,
        vad: VoiceActivityDetector,
        /// The most recent audio (for the pre-roll) and the stream position of its first sample.
        history: VecDeque<f32>,
        history_start: u64,
        total: u64,
    }

    fn make_vad(model: &Path, s: &SegmenterSettings) -> Result<VoiceActivityDetector, String> {
        let cfg = VadModelConfig {
            silero_vad: SileroVadModelConfig {
                model: Some(model.to_string_lossy().to_string()),
                threshold: s.threshold,
                min_silence_duration: s.min_silence,
                min_speech_duration: s.min_speech,
                window_size: 512,
                max_speech_duration: s.max_speech,
            },
            sample_rate: RATE as i32,
            num_threads: 1,
            provider: Some("cpu".into()),
            ..Default::default()
        };
        VoiceActivityDetector::create(&cfg, 60.0)
            .ok_or_else(|| "The voice detector could not be loaded. Try downloading the speech model again.".to_string())
    }

    impl SileroSegmenter {
        fn collect(&mut self) -> Vec<Vec<f32>> {
            let mut out = Vec::new();
            while let Some(segment) = self.vad.front() {
                let start = segment.start().max(0) as u64;
                let mut samples: Vec<f32> = segment.samples().to_vec();
                drop(segment);
                self.vad.pop();
                let pre = (self.settings.pre_roll * RATE as f32) as u64;
                let from = start.saturating_sub(pre).max(self.history_start);
                let mut utterance: Vec<f32> = Vec::with_capacity(samples.len() + (start.saturating_sub(from)) as usize);
                if from < start {
                    let (a, b) = self.history.as_slices();
                    let skip = (from - self.history_start) as usize;
                    let take = (start - from) as usize;
                    utterance.extend(a.iter().chain(b.iter()).skip(skip).take(take));
                }
                utterance.append(&mut samples);
                // A little of what followed, so a quiet last syllable is not cut off.
                let end = start + (utterance.len() - (start.saturating_sub(from)) as usize) as u64;
                let post = (self.settings.post_roll * RATE as f32) as u64;
                let until = (end + post).min(self.total);
                if until > end && end >= self.history_start {
                    let (a, b) = self.history.as_slices();
                    utterance.extend(a.iter().chain(b.iter()).skip((end - self.history_start) as usize).take((until - end) as usize));
                }
                out.push(utterance);
            }
            out
        }

        fn restart(&mut self) {
            match make_vad(&self.model, &self.settings) {
                Ok(v) => self.vad = v,
                Err(_) => {
                    self.vad.reset();
                    self.vad.clear();
                }
            }
            self.history.clear();
            self.history_start = 0;
            self.total = 0;
        }
    }

    impl Segmenter for SileroSegmenter {
        fn feed(&mut self, chunk: &[f32]) -> Segmented {
            if chunk.is_empty() {
                return Segmented { utterances: Vec::new(), speech_in_progress: self.vad.detected() };
            }
            self.history.extend(chunk.iter().copied());
            self.total += chunk.len() as u64;
            // Keep the longest utterance plus the pre-roll and some slack.
            let keep = ((self.settings.max_speech + self.settings.pre_roll + 5.0) * RATE as f32) as usize;
            while self.history.len() > keep {
                self.history.pop_front();
                self.history_start += 1;
            }
            self.vad.accept_waveform(chunk);
            let utterances = self.collect();
            Segmented { utterances, speech_in_progress: self.vad.detected() }
        }

        fn flush(&mut self) -> Vec<Vec<f32>> {
            self.vad.flush();
            let out = self.collect();
            self.restart();
            out
        }

        fn reset(&mut self) {
            self.restart();
        }
    }

    /// Silero VAD with the standard settings (pre-roll 400 ms, pause 600 ms, at most 15 s).
    pub fn create_segmenter() -> Result<Box<dyn Segmenter>, String> {
        create_segmenter_with(&models_dir(), &vad_spec(), SegmenterSettings::default())
    }

    pub fn create_segmenter_with(dir: &Path, vad: &ModelSpec, settings: SegmenterSettings) -> Result<Box<dyn Segmenter>, String> {
        if installed_size_in(dir, vad).is_none() {
            return Err("The voice detector is not downloaded yet. Download a speech model first.".into());
        }
        let model = dir.join(&vad.id).join(VAD_FILE);
        let vad = make_vad(&model, &settings)?;
        Ok(Box::new(SileroSegmenter { model, settings, vad, history: VecDeque::new(), history_start: 0, total: 0 }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_is_complete_and_https() {
        for s in catalogue().iter().chain([&vad_spec()]) {
            assert!(s.url.starts_with("https://github.com/k2-fsa/sherpa-onnx/releases/download/"));
            assert_eq!(s.sha256.len(), 64);
            assert!(s.bytes > 0);
        }
        assert_eq!(model_for_language("de").as_deref(), Some("sherpa-de"));
        assert_eq!(model_for_language("fr"), None);
    }

    #[cfg(feature = "voice-engine")]
    #[test]
    fn engine_list_has_licences() {
        let engines = available_engines();
        assert_eq!(engines.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["sherpa-en", "sherpa-de", "sherpa-ru"]);
        for e in &engines {
            assert!(e.local && e.description.contains("never leaves"));
        }
        assert!(engines[0].description.contains("CC-BY-SA") && engines[0].description.contains("Banafo"));
        assert!(engines[2].license.contains("Apache-2.0"));
    }

    #[test]
    fn unknown_model_is_not_installed() {
        let s = model_status("nope");
        assert!(!s.installed);
        assert!(download_model("nope", &|_, _| {}, &AtomicBool::new(false)).is_err());
    }

    #[cfg(feature = "voice-engine")]
    #[test]
    fn entry_paths_are_checked() {
        assert!(clean_entry_path(Path::new("../evil")).is_err());
        assert!(clean_entry_path(Path::new("top/../../evil")).is_err());
        assert!(clean_entry_path(Path::new("/etc/passwd")).is_err());
        assert!(clean_entry_path(Path::new("top/a:b")).is_err());
        assert_eq!(clean_entry_path(Path::new("top")).unwrap(), None);
        assert_eq!(clean_entry_path(Path::new("top/test_wavs/0.wav")).unwrap(), None);
        assert_eq!(clean_entry_path(Path::new("top/sub/f.onnx")).unwrap(), Some(PathBuf::from("sub/f.onnx")));
    }

    #[cfg(not(feature = "voice-engine"))]
    #[test]
    fn fallback_reports_no_engine() {
        assert_eq!(create_recognizer("sherpa-en", "en").err().unwrap(), NO_ENGINE);
        assert_eq!(create_segmenter().err().unwrap(), NO_ENGINE);
    }
}
