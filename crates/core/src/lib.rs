//! LocalFlow core: everything except the user interface.
//!
//! Rust owns the database, scheduler and filesystem access. Users write
//! automations in Lua, which run in a sandbox that only sees the functions
//! registered in [`lua::api`]. Front-ends (the web server and the desktop app)
//! talk to the [`LocalFlow`] service.

pub mod config;
pub mod db;
pub mod errors;
pub mod lua;
pub mod scheduler;
pub mod watcher;
mod service;

pub use config::CoreConfig;
pub use errors::{CoreError, CoreResult};
pub use service::{AutomationInput, AutomationSummary, CoreEvent, EventHandler, LocalFlow, TestRunResult};
