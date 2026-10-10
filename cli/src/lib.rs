//! `msgq` — durable, topic-addressed message bus over a single SQLite file.
//!
//! The CLI is a thin protocol and UX layer over honker's durable streams.
//! The engine is confined to the `store` module so the record contract and
//! address grammar stay independent of it.

pub mod address;
pub mod commands;
pub mod config;
pub mod error;
pub mod record;
pub mod store;

/// Package version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
