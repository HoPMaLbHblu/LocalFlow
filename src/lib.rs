//! LocalFlow: a local automation server.
//!
//! Rust owns the HTTP server, database, scheduler and filesystem access.
//! Users write automations in Lua, which run in a sandbox that only sees the
//! functions registered in [`lua::api`].

pub mod api;
pub mod db;
pub mod errors;
pub mod lua;
pub mod scheduler;
pub mod state;
