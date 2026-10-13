//! Canonical identity helpers: digests, paths, components (P03).
//!
//! Declared via `#[path]` from `internal_plan.rs` (no `lib.rs` edit).
//! Centralizes canonical serialization, strict parsing, checkout-path
//! normalization, Cargo ID normalization, and generator markers. Group
//! identities live in [`super::identities`], per-task closures in
//! [`super::closure`]; unknown inputs are explicit states, never
//! silent `None`s.

use serde::Serialize;
use velnor_actions_contract::{
    ContractError, canonical_json_bytes, digest_b3, normalize_posix_path, parse_strict_json,
};

/// Explicit unknown marker for unverifiable archive sources.
pub(crate) const UNKNOWN_ARCHIVE_SOURCE: &str = "velnor-unknown-archive-source-v1";
/// SHA-256 of the empty string: explicit unverified-generator marker.
pub(crate) const UNRESOLVED_GENERATOR_SHA: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// BLAKE3 digest over canonical JSON bytes: the single digest function.
///
/// # Errors
///
/// Returns [`ContractError`] when canonical serialization fails.
pub(crate) fn canonical_digest<T: Serialize>(value: &T) -> Result<String, ContractError> {
    Ok(digest_b3(&canonical_json_bytes(value)?))
}

/// Strict JSON parse: duplicate keys are rejected, never last-wins.
///
/// # Errors
///
/// Returns [`ContractError`] for malformed JSON or duplicate keys.
pub(crate) fn parse_canonical_json(text: &str) -> Result<serde_json::Value, ContractError> {
    parse_strict_json(text)
}

/// Normalize a checkout path: repo-relative, `/` separators, explicit rejects.
///
/// Case and Unicode are preserved byte-for-byte; empty, absolute,
/// traversing, NUL/control-carrying, and backslash paths are rejected.
///
/// # Errors
///
/// Returns [`ContractError`] for malformed checkout paths.
pub(crate) fn normalize_checkout_path(path: &str) -> Result<String, ContractError> {
    if path.is_empty() {
        return Err(ContractError::identity("path", "empty_path"));
    }
    if path.contains('\0') || path.chars().any(char::is_control) {
        return Err(ContractError::identity("path", "control_characters"));
    }
    if path.contains('\\') {
        return Err(ContractError::identity("path", "backslash_separator"));
    }
    normalize_posix_path(path)
}

/// Stable component identity from a raw Cargo package ID plus manifest.
///
/// Raw Cargo IDs are diagnostic-only: absolute checkout paths embedded in
/// `path+file://` or `registry+` qualifiers never enter an identity.
/// Plain IDs (`demo`, `demo 0.1.0`) pass through verbatim; qualified IDs
/// reduce to their trailing `name@version` fragment; empty IDs anchor to
/// the owning manifest (or `workspace` at the root).
pub(crate) fn normalized_component_id(package_id: &str, manifest: &str) -> String {
    if package_id.is_empty() {
        let root = manifest
            .rsplit_once('/')
            .map_or("", |(dir, _)| if dir.is_empty() { "" } else { dir });
        if root.is_empty() {
            return "workspace".to_owned();
        }
        return root.to_owned();
    }
    if let Some(fragment) = package_id.rsplit('#').next()
        && fragment.contains('@')
        && package_id.contains("://")
    {
        return fragment.to_owned();
    }
    package_id.to_owned()
}

/// Release triple for the build host; unknown pairs keep `{arch}-{os}`.
pub(crate) fn map_release_triple(arch: &str, os: &str) -> String {
    match (arch, os) {
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        _ => return format!("{arch}-{os}"),
    }
    .to_owned()
}

/// Content identity of the running executable, computed in-process.
///
/// BLAKE3 over the executable bytes; no subprocess is spawned (the
/// orchestrator never spawns processes). The `b3-` prefix distinguishes
/// native hashes from release SHA-256 pins, which no in-process
/// algorithm here can reproduce; exact executable-against-release
/// verification awaits a SHA-256 facility (follow-up).
pub(crate) fn current_exe_content_digest() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let bytes = std::fs::read(exe).ok()?;
    Some(digest_b3(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_bytes_ignore_key_order() {
        let left = serde_json::json!({"b": 1, "a": [1, 2]});
        let right = serde_json::json!({"a": [1, 2], "b": 1});
        assert_eq!(
            canonical_digest(&left).expect("digest"),
            canonical_digest(&right).expect("digest")
        );
        assert!(parse_canonical_json(r#"{"a": 1, "a": 2}"#).is_err());
        assert!(parse_canonical_json(r#"{"a": 1}"#).is_ok());
    }

    #[test]
    fn checkout_paths_preserve_case_and_unicode() {
        assert_eq!(
            normalize_checkout_path("Crates/Äpfel/Cargo.toml").expect("unicode"),
            "Crates/Äpfel/Cargo.toml"
        );
        for bad in ["", "/abs/path", "a/../b", "a\\b", "a\0b", "a\nb"] {
            assert!(normalize_checkout_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn component_ids_shed_checkout_paths() {
        assert_eq!(normalized_component_id("demo", "Cargo.toml"), "demo");
        assert_eq!(
            normalized_component_id("demo 0.1.0", "Cargo.toml"),
            "demo 0.1.0"
        );
        assert_eq!(
            normalized_component_id("path+file:///Users/dev/proj#demo@0.1.0", "Cargo.toml"),
            "demo@0.1.0"
        );
        assert_eq!(
            normalized_component_id(
                "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0",
                "Cargo.toml"
            ),
            "serde@1.0.0"
        );
        assert_eq!(normalized_component_id("", "Cargo.toml"), "workspace");
        assert_eq!(
            normalized_component_id("", "crates/a/Cargo.toml"),
            "crates/a"
        );
    }

    #[test]
    fn triples_map_and_exe_binds_content() {
        assert_eq!(
            map_release_triple("x86_64", "linux"),
            "x86_64-unknown-linux-gnu"
        );
        assert_eq!(map_release_triple("riscv64", "linux"), "riscv64-linux");
        let digest = current_exe_content_digest().expect("exe readable");
        assert!(velnor_actions_contract::validate_digest(&digest).is_ok());
        assert_ne!(digest, digest_b3(b"other-bytes"));
        assert!(!UNRESOLVED_GENERATOR_SHA.bytes().all(|b| b == b'0'));
    }
}
