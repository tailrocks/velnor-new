//! Java/Gradle stack adapter: static, process-free evidence.
//!
//! This crate reads repository files and returns typed findings. It launches
//! no process, runs no Gradle code and renders no workflow text. It owns:
//!
//! - the HD-4a version grammars ([`version`]);
//! - the wrapper gates of the stack contract ([`wrapper`], [`catalog`]);
//! - the static `settings.gradle(.kts)` project model ([`settings`]) and the
//!   project dependency edges of build scripts ([`edges`]);
//! - the dependency-integrity check ([`integrity`]): Gradle dependency
//!   locking and verification metadata.
//!
//! Every finding carries a stable machine-readable code.

pub mod analysis;
pub mod catalog;
pub mod edges;
pub mod integrity;
pub mod lex;
pub mod settings;
pub mod version;
pub mod wrapper;

/// Stable identifier for the Gradle stack.
pub const STACK_ID: &str = "gradle";
