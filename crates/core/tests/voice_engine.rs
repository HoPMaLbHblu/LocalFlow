//! Voice engine tests: model download/verification/extraction with injected sources, the cpal
//! error mapping, sample conversion, and (ignored) live tests with the real models and speech made
//! by Windows TTS. No microphone is ever opened; audio comes from fakes and generated WAV files.
#![cfg(feature = "voice-engine")]

use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use localflow_core::voice::audio::{map_stream_error, Mono16k};
use localflow_core::voice::engine::*;
use localflow_core::voice::AudioEvent;
use sha2::{Digest, Sha256};

// ---- helpers to build archives ------------------------------------------------------------------

enum Item<'a> {
    File(&'a str, Vec<u8>),
    Dir(&'a str),
    Symlink(&'a str, &'a str),
}

/// A .tar.bz2 with exactly these entries. Names are written raw so `..` and absolute paths work.
fn make_archive(items: &[Item]) -> Vec<u8> {
    let mut tar_bytes = Vec::new();
    {
        let mut b = tar::Builder::new(&mut tar_bytes);
        for item in items {
            let mut h = tar::Header::new_gnu();
            let (name, data, kind, link) = match item {
                Item::File(n, d) => (*n, d.clone(), tar::EntryType::Regular, None),
                Item::Dir(n) => (*n, Vec::new(), tar::EntryType::Directory, None),
                Item::Symlink(n, t) => (*n, Vec::new(), tar::EntryType::Symlink, Some(*t)),
            };
            {
                let gnu = h.as_gnu_mut().unwrap();
                gnu.name[..name.len()].copy_from_slice(name.as_bytes());
                if let Some(t) = link {
                    gnu.linkname[..t.len()].copy_from_slice(t.as_bytes());
                }
            }
            h.set_entry_type(kind);
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append(&h, &data[..]).unwrap();
        }
        b.finish().unwrap();
    }
    let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    enc.write_all(&tar_bytes).unwrap();
    enc.finish().unwrap()
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn spec_for(id: &str, archive: &[u8], packaging: Packaging, required: &[&str]) -> ModelSpec {
    ModelSpec {
        id: id.into(),
        name: format!("Test {id}"),
        languages: vec!["en".into()],
        url: format!("https://example.invalid/{id}"),
        bytes: archive.len() as u64,
        sha256: sha(archive),
        packaging,
        required: required.iter().map(|s| s.to_string()).collect(),
        max_extracted_bytes: 1024 * 1024,
        license: "test".into(),
        attribution: "test".into(),
    }
}

fn good_items() -> Vec<Item<'static>> {
    vec![
        Item::Dir("top/"),
        Item::File("top/encoder.onnx", vec![1; 5000]),
        Item::File("top/tokens.txt", b"a 0\nb 1\n".to_vec()),
        Item::Dir("top/test_wavs/"),
        Item::File("top/test_wavs/0.wav", vec![9; 100]),
    ]
}

struct MemFetcher {
    data: Vec<u8>,
    announce: Option<u64>,
}

impl Fetcher for MemFetcher {
    fn open(&self, _url: &str) -> Result<Download, String> {
        Ok(Download { reader: Box::new(Cursor::new(self.data.clone())), length: self.announce.or(Some(self.data.len() as u64)) })
    }
}

/// Serves the bytes slowly and sets `cancel` after `cancel_after` bytes were read.
struct CancellingFetcher {
    data: Vec<u8>,
    cancel: Arc<AtomicBool>,
    cancel_after: usize,
}

struct CancellingReader {
    inner: Cursor<Vec<u8>>,
    cancel: Arc<AtomicBool>,
    cancel_after: usize,
    read: usize,
}

impl Read for CancellingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let lim = buf.len().min(16 * 1024);
        let n = self.inner.read(&mut buf[..lim])?;
        self.read += n;
        if self.read >= self.cancel_after {
            self.cancel.store(true, Ordering::Relaxed);
        }
        Ok(n)
    }
}

impl Fetcher for CancellingFetcher {
    fn open(&self, _url: &str) -> Result<Download, String> {
        Ok(Download {
            length: Some(self.data.len() as u64),
            reader: Box::new(CancellingReader {
                inner: Cursor::new(self.data.clone()),
                cancel: self.cancel.clone(),
                cancel_after: self.cancel_after,
                read: 0,
            }),
        })
    }
}

fn entries(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn vad_bytes() -> Vec<u8> {
    vec![7u8; 4000]
}

fn vad_spec_for(bytes: &[u8]) -> ModelSpec {
    spec_for("silero-vad", bytes, Packaging::SingleFile, &["silero_vad.onnx"])
}

// ---- download and install ---------------------------------------------------------------------------

#[test]
fn installs_verifies_and_reports_progress() {
    let dir = tempfile::tempdir().unwrap();
    let archive = make_archive(&good_items());
    let spec = spec_for("sherpa-test", &archive, Packaging::TarBz2, &["encoder.onnx", "tokens.txt"]);
    let vb = vad_bytes();
    let vad = vad_spec_for(&vb);
    assert!(!model_status_in(dir.path(), &spec, &vad).installed);

    // One fetcher serves both files, keyed by the URL.
    struct Both(Vec<u8>, Vec<u8>);
    impl Fetcher for Both {
        fn open(&self, url: &str) -> Result<Download, String> {
            let data = if url.ends_with("silero-vad") { self.1.clone() } else { self.0.clone() };
            Ok(Download { length: Some(data.len() as u64), reader: Box::new(Cursor::new(data)) })
        }
    }
    let last = AtomicU64::new(0);
    let total_seen = AtomicU64::new(0);
    download_into(
        dir.path(),
        &spec,
        &vad,
        &Both(archive.clone(), vb.clone()),
        &|done, total| {
            assert!(done <= total);
            assert!(done >= last.swap(done, Ordering::Relaxed), "progress went backwards");
            total_seen.store(total, Ordering::Relaxed);
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(total_seen.load(Ordering::Relaxed), (archive.len() + vb.len()) as u64);
    assert_eq!(last.load(Ordering::Relaxed), total_seen.load(Ordering::Relaxed));
    let status = model_status_in(dir.path(), &spec, &vad);
    assert!(status.installed);
    assert_eq!(status.size_bytes, 5000 + 8 + 4000);
    // test_wavs is skipped, the top-level folder is stripped, no temp files remain.
    assert_eq!(entries(dir.path()), ["sherpa-test", "silero-vad"]);
    assert!(!dir.path().join("sherpa-test").join("test_wavs").exists());
    assert!(dir.path().join("sherpa-test").join("encoder.onnx").is_file());

    // Truncating an installed file makes the status "not installed" (it is verified, not just present).
    fs::write(dir.path().join("sherpa-test").join("encoder.onnx"), b"short").unwrap();
    assert!(!model_status_in(dir.path(), &spec, &vad).installed);
    // Re-downloading repairs it; with the VAD model deleted only that one is fetched again.
    download_into(dir.path(), &spec, &vad, &Both(archive.clone(), vb.clone()), &|_, _| {}, &AtomicBool::new(false)).unwrap();
    assert!(model_status_in(dir.path(), &spec, &vad).installed);
    fs::remove_dir_all(dir.path().join("silero-vad")).unwrap();
    assert!(!model_status_in(dir.path(), &spec, &vad).installed);

    // Removing the only speech model also removes the VAD model.
    download_into(dir.path(), &spec, &vad, &Both(archive, vb), &|_, _| {}, &AtomicBool::new(false)).unwrap();
    remove_from(dir.path(), &spec).unwrap();
    assert!(entries(dir.path()).is_empty());
}

#[test]
fn tampered_download_is_rejected_and_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let archive = make_archive(&good_items());
    let spec = spec_for("sherpa-test", &archive, Packaging::TarBz2, &["encoder.onnx"]);
    let mut tampered = archive.clone();
    let mid = tampered.len() / 2;
    tampered[mid] ^= 0x55;
    let vb = vad_bytes();
    let vad = vad_spec_for(&vb);
    // The VAD model is fine, but the speech model is tampered: the speech model must not be installed.
    struct Mixed(Vec<u8>, Vec<u8>);
    impl Fetcher for Mixed {
        fn open(&self, url: &str) -> Result<Download, String> {
            let d = if url.ends_with("silero-vad") { self.1.clone() } else { self.0.clone() };
            Ok(Download { length: Some(d.len() as u64), reader: Box::new(Cursor::new(d)) })
        }
    }
    let err = download_into(dir.path(), &spec, &vad, &Mixed(tampered, vb), &|_, _| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(err.contains("checksum"), "{err}");
    assert!(!model_status_in(dir.path(), &spec, &vad).installed);
    assert!(!dir.path().join("sherpa-test").exists());
    assert!(entries(dir.path()).iter().all(|n| !n.starts_with(".tmp-")), "{:?}", entries(dir.path()));
}

#[test]
fn wrong_announced_length_and_oversize_downloads_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let archive = make_archive(&good_items());
    let spec = spec_for("sherpa-test", &archive, Packaging::TarBz2, &["encoder.onnx"]);
    let vb = vad_bytes();
    let vad = vad_spec_for(&vb);
    // Make the VAD installed so only the speech model is fetched.
    download_into(
        dir.path(),
        &vad,
        &vad,
        &MemFetcher { data: vb, announce: None },
        &|_, _| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    let err = install_with(&spec, dir.path(), &MemFetcher { data: archive.clone(), announce: Some(5) }, 0, 1, &|_, _| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(err.contains("announced"), "{err}");
    let mut longer = archive.clone();
    longer.extend_from_slice(&[0; 100]);
    let err = install_with(&spec, dir.path(), &MemFetcher { data: longer, announce: Some(archive.len() as u64) }, 0, 1, &|_, _| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(err.contains("larger"), "{err}");
    let err = install_with(&spec, dir.path(), &MemFetcher { data: archive[..archive.len() - 10].to_vec(), announce: Some(archive.len() as u64) }, 0, 1, &|_, _| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(err.contains("incomplete"), "{err}");
    assert_eq!(entries(dir.path()), ["silero-vad"]);
}

fn assert_rejected(items: Vec<Item>, expect: &str) {
    let dir = tempfile::tempdir().unwrap();
    let archive = make_archive(&items);
    let spec = spec_for("sherpa-test", &archive, Packaging::TarBz2, &["encoder.onnx"]);
    // Checksums match: the archive itself must be refused, after verification.
    let err = install_with(&spec, dir.path(), &MemFetcher { data: archive, announce: None }, 0, 1, &|_, _| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(err.contains(expect), "expected '{expect}' in '{err}'");
    assert!(entries(dir.path()).is_empty(), "left behind: {:?}", entries(dir.path()));
}

#[test]
fn path_traversal_absolute_and_link_archives_are_rejected() {
    assert_rejected(vec![Item::File("top/../../evil.txt", vec![1])], "unsafe path");
    assert_rejected(vec![Item::File("../evil.txt", vec![1])], "unsafe path");
    assert_rejected(vec![Item::File("/abs/evil.txt", vec![1])], "unsafe path");
    assert_rejected(vec![Item::File("top/C:evil.txt", vec![1])], "unsafe path");
    assert_rejected(vec![Item::File("top/encoder.onnx", vec![1]), Item::Symlink("top/link", "/etc/passwd")], "link");
    assert_rejected(vec![Item::File("top/a.onnx", vec![1]), Item::File("top/a.onnx", vec![2])], "twice");
    // A valid archive that lacks a required file is not installed either.
    assert_rejected(vec![Item::File("top/other.txt", vec![1])], "lacks");
}

#[test]
fn archive_bombs_are_limited() {
    let dir = tempfile::tempdir().unwrap();
    let archive = make_archive(&[Item::File("top/encoder.onnx", vec![0; 3 * 1024 * 1024])]);
    let spec = spec_for("sherpa-test", &archive, Packaging::TarBz2, &["encoder.onnx"]); // limit 1 MiB
    let err = install_with(&spec, dir.path(), &MemFetcher { data: archive, announce: None }, 0, 1, &|_, _| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(err.contains("more than the allowed"), "{err}");
    assert!(entries(dir.path()).is_empty());
}

#[test]
fn cancelling_mid_download_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    // Random-ish data so the archive is big enough to span many reads.
    let mut noise = Vec::new();
    let mut x: u32 = 12345;
    for _ in 0..600_000 {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        noise.push((x >> 24) as u8);
    }
    let mut items = good_items();
    items.push(Item::File("top/big.bin", noise));
    let archive = make_archive(&items);
    let mut spec = spec_for("sherpa-test", &archive, Packaging::TarBz2, &["encoder.onnx"]);
    spec.max_extracted_bytes = 4 * 1024 * 1024;
    let cancel = Arc::new(AtomicBool::new(false));
    let fetcher = CancellingFetcher { data: archive.clone(), cancel: cancel.clone(), cancel_after: archive.len() / 3 };
    let seen = AtomicU64::new(0);
    let started = Instant::now();
    let err = install_with(&spec, dir.path(), &fetcher, 0, spec.bytes, &|d, _| seen.store(d, Ordering::Relaxed), &cancel).unwrap_err();
    assert_eq!(err, CANCELLED);
    assert!(started.elapsed().as_secs() < 5);
    assert!(seen.load(Ordering::Relaxed) < spec.bytes, "stopped before the end");
    assert!(entries(dir.path()).is_empty(), "left behind: {:?}", entries(dir.path()));

    // Cancelled before anything starts: no request, nothing created.
    let err = install_with(&spec, dir.path(), &MemFetcher { data: archive, announce: None }, 0, 1, &|_, _| {}, &AtomicBool::new(true)).unwrap_err();
    assert_eq!(err, CANCELLED);
    assert!(entries(dir.path()).is_empty());
}

#[test]
fn failed_update_keeps_the_old_installation() {
    let dir = tempfile::tempdir().unwrap();
    let archive = make_archive(&good_items());
    let spec = spec_for("sherpa-test", &archive, Packaging::TarBz2, &["encoder.onnx"]);
    install_with(&spec, dir.path(), &MemFetcher { data: archive.clone(), announce: None }, 0, 1, &|_, _| {}, &AtomicBool::new(false)).unwrap();
    assert!(installed_size_in(dir.path(), &spec).is_some());
    let mut bad = archive.clone();
    let n = bad.len() / 2;
    bad[n] ^= 1;
    assert!(install_with(&spec, dir.path(), &MemFetcher { data: bad, announce: None }, 0, 1, &|_, _| {}, &AtomicBool::new(false)).is_err());
    assert!(installed_size_in(dir.path(), &spec).is_some());
    assert_eq!(entries(dir.path()), ["sherpa-test"]);
}

#[test]
fn stale_temp_files_are_cleaned_and_models_kept() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".tmp-x.dir")).unwrap();
    fs::write(dir.path().join(".tmp-x.part"), b"x").unwrap();
    fs::create_dir_all(dir.path().join("sherpa-keep")).unwrap();
    clean_stale_temp_files(dir.path());
    assert_eq!(entries(dir.path()), ["sherpa-keep"]);
}

#[test]
fn https_fetcher_refuses_plain_http() {
    let err = HttpsFetcher.open("http://example.com/model.tar.bz2").err().unwrap();
    assert!(err.contains("HTTPS"));
}

#[test]
fn not_installed_models_cannot_be_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let spec = catalogue().remove(0);
    let err = create_recognizer_in(dir.path(), &spec, &vad_spec()).err().unwrap();
    assert!(err.contains("not downloaded"));
    assert!(create_segmenter_with(dir.path(), &vad_spec(), SegmenterSettings::default()).is_err());
}

// ---- capture: error mapping and conversion ------------------------------------------------------------

#[test]
fn stream_errors_map_to_friendly_events() {
    use cpal::ErrorKind as K;
    let ev = |k: K| map_stream_error(&cpal::Error::new(k));
    assert_eq!(ev(K::DeviceNotAvailable), Some(AudioEvent::Disconnected));
    assert_eq!(ev(K::StreamInvalidated), Some(AudioEvent::Disconnected));
    assert_eq!(ev(K::DeviceChanged), Some(AudioEvent::Disconnected));
    assert_eq!(ev(K::Xrun), None);
    assert_eq!(ev(K::RealtimeDenied), None);
    let text = |k: K| match ev(k) {
        Some(AudioEvent::Error(t)) => t,
        other => panic!("{k:?} -> {other:?}"),
    };
    assert!(text(K::PermissionDenied).to_lowercase().contains("microphone access was denied"));
    assert!(text(K::PermissionDenied).contains("Privacy"));
    assert!(text(K::DeviceBusy).contains("in use"));
    assert!(text(K::HostUnavailable).contains("audio system"));
    assert!(text(K::BackendError).contains("microphone failed"));
}

fn sine(rate: u32, channels: usize, seconds: f32, hz: f32) -> Vec<f32> {
    let n = (rate as f32 * seconds) as usize;
    let mut v = Vec::with_capacity(n * channels);
    for i in 0..n {
        let s = 0.5 * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin();
        for _ in 0..channels {
            v.push(s);
        }
    }
    v
}

#[test]
fn conversion_gives_16k_mono_chunks_of_80ms() {
    for (rate, channels) in [(48_000u32, 2usize), (44_100, 1), (16_000, 2), (96_000, 2)] {
        let mut conv = Mono16k::new(rate, channels as u16).unwrap();
        let input = sine(rate, channels, 2.0, 440.0);
        let mut out: Vec<f32> = Vec::new();
        // Feed in callback-sized pieces, as cpal does.
        for piece in input.chunks(480 * channels) {
            for chunk in conv.push(piece) {
                assert_eq!(chunk.len(), 1280);
                out.extend(chunk);
            }
        }
        // About 2 s at 16 kHz (minus the resampler's delay and the unsent remainder).
        assert!(out.len() > 30_000 && out.len() <= 32_000, "{rate}/{channels}: {}", out.len());
        // The tone survives (RMS of a 0.5 sine = 0.354) and is not clipped or silent.
        let body = &out[4000..];
        let rms = (body.iter().map(|s| s * s).sum::<f32>() / body.len() as f32).sqrt();
        assert!((rms - 0.354).abs() < 0.03, "{rate}/{channels}: rms {rms}");
    }
    // Stereo with opposite channels downmixes to silence (proper averaging).
    let mut conv = Mono16k::new(16_000, 2).unwrap();
    let opposite: Vec<f32> = (0..4000).flat_map(|i| [0.5 * (i as f32 * 0.1).sin(), -0.5 * (i as f32 * 0.1).sin()]).collect();
    for chunk in conv.push(&opposite) {
        assert!(chunk.iter().all(|s| s.abs() < 1e-6));
    }
}

// ---- live tests: real models, speech from Windows TTS -----------------------------------------------------

fn read_wav16(path: &Path) -> Vec<f32> {
    let b = fs::read(path).unwrap();
    assert_eq!(&b[..4], b"RIFF");
    let mut i = 12;
    while i + 8 <= b.len() {
        let size = u32::from_le_bytes(b[i + 4..i + 8].try_into().unwrap()) as usize;
        if &b[i..i + 4] == b"data" {
            let end = (i + 8 + size).min(b.len());
            return b[i + 8..end].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect();
        }
        i += 8 + size + (size & 1);
    }
    panic!("no data chunk");
}

fn powershell(script: &str, env: &[(&str, &str)]) -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// (voice name, culture) of the installed Windows TTS voices; empty when System.Speech is unavailable.
fn voices() -> Vec<(String, String)> {
    let script = "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
                  $s.GetInstalledVoices() | ForEach-Object { $_.VoiceInfo.Name + '|' + $_.VoiceInfo.Culture.Name }";
    powershell(script, &[])
        .map(|o| {
            o.lines()
                .filter_map(|l| l.trim().split_once('|').map(|(a, b)| (a.to_string(), b.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Synthesise phrases (16 kHz mono) with a Windows voice. `None` if TTS is unavailable.
fn synth(voice: &str, phrases: &[&str], dir: &Path) -> Option<Vec<Vec<f32>>> {
    let script = "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
        $s.SelectVoice($env:LF_VOICE); \
        $f = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono); \
        $i = 0; foreach ($t in $env:LF_TEXTS.Split('|')) { $s.SetOutputToWaveFile((Join-Path $env:LF_OUT (\"$i.wav\")), $f); $s.Speak($t); $s.SetOutputToNull(); $i++ }; $s.Dispose()";
    let texts = phrases.join("|");
    powershell(script, &[("LF_VOICE", voice), ("LF_TEXTS", &texts), ("LF_OUT", &dir.to_string_lossy())])?;
    (0..phrases.len()).map(|i| {
        let p = dir.join(format!("{i}.wav"));
        p.exists().then(|| read_wav16(&p))
    }).collect()
}

fn normalise(s: &str) -> String {
    s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn live_data_dir() -> PathBuf {
    // One shared temp folder for the live tests, so the models are downloaded once per run.
    let dir = std::env::var_os("LF_LIVE_DIR").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("localflow-voice-live"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Download (real HTTPS) the model into LOCALFLOW_DATA_DIR if missing; returns seconds taken.
fn ensure_model(id: &str) -> f64 {
    std::env::set_var("LOCALFLOW_DATA_DIR", live_data_dir());
    let t = Instant::now();
    let calls = AtomicU64::new(0);
    let last = AtomicU64::new(0);
    let was = model_status(id).installed;
    download_model(id, &|d, _| { calls.fetch_add(1, Ordering::Relaxed); last.store(d, Ordering::Relaxed); }, &AtomicBool::new(false)).unwrap();
    let secs = t.elapsed().as_secs_f64();
    let status = model_status(id);
    assert!(status.installed, "{id} not installed after download");
    println!("[live] {id}: download step {:.1}s (already installed: {was}), {} progress calls, {} MB on disk", secs, calls.load(Ordering::Relaxed), status.size_bytes / 1_000_000);
    secs
}

fn run_recognition(model: &str, lang: &str, voice_filter: impl Fn(&(String, String)) -> bool, phrases: &[&str], required_ratio: f32) {
    ensure_model(model);
    let vs: Vec<_> = voices().into_iter().filter(|v| voice_filter(v)).collect();
    if vs.is_empty() {
        println!("[live] no matching Windows TTS voice: recognition test skipped");
        return;
    }
    let t = Instant::now();
    let mut rec = create_recognizer(model, lang).unwrap();
    println!("[live] model load {:.2}s ({})", t.elapsed().as_secs_f64(), rec.name());
    let (mut ok, mut all, mut audio_s, mut proc_s) = (0, 0, 0.0f64, 0.0f64);
    for (voice, _) in &vs {
        let tmp = tempfile::tempdir().unwrap();
        let Some(clips) = synth(voice, phrases, tmp.path()) else { println!("[live] TTS failed for {voice}"); continue };
        for (clip, phrase) in clips.iter().zip(phrases) {
            let t = Instant::now();
            let r = rec.transcribe(clip, lang).unwrap();
            let dt = t.elapsed().as_secs_f64();
            audio_s += clip.len() as f64 / 16000.0;
            proc_s += dt;
            all += 1;
            let good = normalise(&r.text) == normalise(phrase);
            ok += good as u32;
            println!("[live] {voice:>8} {:>5.2}s {:>4.0} ms  {:<24} => {:<28} {}", clip.len() as f64 / 16000.0, dt * 1000.0, phrase, r.text, if good { "OK" } else { "MISS" });
            assert_eq!(r.text, r.text.to_lowercase());
            assert_eq!(r.confidence, None);
        }
    }
    println!("[live] {model}: {ok}/{all} exact, real-time factor {:.3} (processing {:.2}s for {:.1}s audio)", proc_s / audio_s, proc_s, audio_s);
    assert!(all > 0 && ok as f32 >= required_ratio * all as f32, "accuracy {ok}/{all}");
    // Silence and empty input decode to nothing, without errors.
    assert_eq!(rec.transcribe(&[], lang).unwrap().text, "");
    assert_eq!(rec.transcribe(&vec![0.0; 16000], lang).unwrap().text, "");
}

#[test]
#[ignore = "downloads the English model (about 57 MB) and uses Windows TTS"]
fn live_english_download_and_recognition() {
    run_recognition(
        "sherpa-en",
        "en",
        |(_, c)| c.starts_with("en"),
        &["run backup", "stop", "dark mode on", "open chrome", "volume up", "take a screenshot", "what can I say", "yes"],
        0.75,
    );
}

#[test]
#[ignore = "downloads the Russian model (about 24 MB) and uses Windows TTS"]
fn live_russian_download_and_recognition() {
    run_recognition(
        "sherpa-ru",
        "ru",
        |(_, c)| c.starts_with("ru"),
        &["запусти резервное копирование", "стоп", "включи тёмную тему", "громкость выше"],
        0.5,
    );
}

#[test]
#[ignore = "downloads the German model (about 58 MB)"]
fn live_german_download_and_load() {
    ensure_model("sherpa-de");
    let mut rec = create_recognizer("sherpa-de", "de").unwrap();
    // German voices are rarely installed; a bundled sample WAV may be passed in.
    if let Some(path) = std::env::var_os("LF_DE_WAV") {
        let r = rec.transcribe(&read_wav16(Path::new(&path)), "de").unwrap();
        println!("[live] de sample => {}", r.text);
        assert!(!r.text.is_empty());
    }
    assert_eq!(rec.transcribe(&vec![0.0; 16000], "de").unwrap().text, "");
}

#[test]
#[ignore = "downloads the English model (about 57 MB) and uses Windows TTS"]
fn live_segmenter_cuts_utterances_with_pre_roll() {
    ensure_model("sherpa-en");
    let vs = voices();
    let Some((voice, _)) = vs.iter().find(|(_, c)| c.starts_with("en")) else { println!("[live] no English voice: skipped"); return };
    let tmp = tempfile::tempdir().unwrap();
    let phrases = ["run backup", "dark mode on", "stop"];
    let Some(clips) = synth(voice, &phrases, tmp.path()) else { return };

    let silence = |s: f32| vec![0.0f32; (16000.0 * s) as usize];
    let mut stream = silence(1.0);
    let mut starts = Vec::new();
    for clip in &clips {
        starts.push(stream.len());
        stream.extend(clip);
        stream.extend(silence(1.3));
    }

    println!("[live] clip lengths {:?} starts {:?}", clips.iter().map(|c| c.len()).collect::<Vec<_>>(), starts);
    let mut seg = create_segmenter().unwrap();
    let mut utterances: Vec<Vec<f32>> = Vec::new();
    let mut saw_progress = false;
    let t = Instant::now();
    for chunk in stream.chunks(1280) {
        let s = seg.feed(chunk);
        saw_progress |= s.speech_in_progress;
        utterances.extend(s.utterances);
    }
    let vad_s = t.elapsed().as_secs_f64();
    println!("[live] VAD: {} utterances from {:.1}s audio in {:.0} ms (RTF {:.4})", utterances.len(), stream.len() as f64 / 16000.0, vad_s * 1000.0, vad_s / (stream.len() as f64 / 16000.0));
    assert!(saw_progress);
    println!("[live] utterance lengths {:?}", utterances.iter().map(Vec::len).collect::<Vec<_>>());
    assert_eq!(utterances.len(), 3, "lengths: {:?}", utterances.iter().map(Vec::len).collect::<Vec<_>>());
    let mut rec = create_recognizer("sherpa-en", "en").unwrap();
    for (u, phrase) in utterances.iter().zip(phrases) {
        let text = rec.transcribe(u, "en").unwrap().text;
        println!("[live] utterance {:.2}s => {text}", u.len() as f64 / 16000.0);
        assert_eq!(normalise(&text).replace(' ', ""), normalise(phrase).replace(' ', ""));
        // Pre-roll: the utterance starts with some quiet lead-in, not mid-word, and stays short.
        assert!(u.len() < 16000 * 4);
    }

    // flush() hands over the speech heard so far although the speaker never paused.
    seg.reset();
    let mut cut = silence(0.6);
    // Only the speech itself (the TTS clip ends with more silence than the segmenter's pause).
    let last_loud = clips[1].iter().rposition(|s| s.abs() > 0.01).unwrap();
    cut.extend(&clips[1][..last_loud + 800]);
    let mut got = Vec::new();
    for chunk in cut.chunks(1280) {
        got.extend(seg.feed(chunk).utterances);
    }
    assert!(got.is_empty(), "no pause yet, so no utterance yet");
    let flushed = seg.flush();
    assert_eq!(flushed.len(), 1);
    assert_eq!(normalise(&rec.transcribe(&flushed[0], "en").unwrap().text), "dark mode on");
    println!("[live] flushed utterance {:.2}s", flushed[0].len() as f64 / 16000.0);
    // After flush the segmenter is clean and flushing again gives nothing.
    assert!(seg.flush().is_empty());

    // Pure silence and low noise produce nothing.
    seg.reset();
    let mut x: u32 = 1;
    let noise: Vec<f32> = (0..16000 * 5).map(|_| { x = x.wrapping_mul(1664525).wrapping_add(1013904223); ((x >> 16) as f32 / 65536.0 - 0.5) * 0.004 }).collect();
    let mut n = 0;
    for chunk in noise.chunks(1280) {
        n += seg.feed(chunk).utterances.len();
    }
    assert_eq!(n + seg.flush().len(), 0);
    let _ = starts;
}
