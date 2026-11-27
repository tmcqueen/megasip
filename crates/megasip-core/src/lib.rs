//! MegaSIP's toolkit-independent core: the SIP engine, media, settings, the account lock
//! table, IPC between windows, secrets, logging and the headless `--cli` mode. Frontends
//! (today the Qt app in `megasip-qt`) build on this crate.

pub mod accounticon;
pub mod cli;
pub mod config;
pub mod db;
pub mod debug;
pub mod engine;
pub mod ipc;
pub mod logging;
pub mod media;
pub mod secrets;
pub mod store;

use std::sync::OnceLock;

/// `--source`: what to send instead of the microphone.
pub static SOURCE: OnceLock<media::source::Source> = OnceLock::new();
