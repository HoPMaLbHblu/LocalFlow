//! Speech engines and their models. OWNER: audio agent. Placeholder with the agreed interface.

use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use super::Recognizer;

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

pub fn available_engines() -> Vec<EngineInfo> {
    Vec::new()
}

pub fn model_status(engine: &str) -> ModelStatus {
    ModelStatus { engine: engine.into(), installed: false, size_bytes: 0 }
}

/// Download and verify (checksum) the model. `progress(done, total)`; stops early if `cancel` is set.
pub fn download_model(_engine: &str, _progress: &dyn Fn(u64, u64), _cancel: &AtomicBool) -> Result<(), String> {
    Err("not implemented yet".into())
}

pub fn remove_model(_engine: &str) -> Result<(), String> {
    Ok(())
}

pub fn create_recognizer(_engine: &str, _language: &str) -> Result<Box<dyn Recognizer>, String> {
    Err("no speech engine is available yet".into())
}
