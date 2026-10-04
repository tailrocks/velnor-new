//! Integrity evidence for the pinned anonymous release-plz preparation tool.
//!
//! Pin: CLI `0.3.169` (crates.io `max_stable_version` plus tag
//! `release-plz-v0.3.169`, published 2026-09-19; rechecked 2026-09-30).
//! Install: mise `release-plz` shorthand via the aqua backend to the
//! prebuilt GitHub tarball; `cargo:release-plz` fallback compiles the
//! checksum-pinned `.crate`. Upstream publishes no per-asset SHA256 or
//! sigstore evidence, so the version plus crate checksum is the anchor.
//! The owned preparation closure supplies the anonymous `update` command.

/// Full crates.io sha256 cksum of `release-plz 0.3.169` (API plus sparse index agree).
pub const RELEASE_PLZ_CKSUM: &str =
    "2f7a1b17465db464a28627bae7832ff7eb9b5f29b4fe89048b4dde8da1f567e5";
