//! Narrow external dependency ownership and feature policy.

use std::error::Error;

use crate::impl_repo_deps::archive_deps::reviewed_archive_dependency;
use crate::impl_repo_policy::{MEMBERS, dep_key, dep_lines, dep_referenced, manifest};

#[test]
fn external_deps_allowlisted_used_and_narrow() -> Result<(), Box<dyn Error>> {
    let allowed = [
        "serde",
        "serde_json",
        "toml",
        "cargo_metadata",
        "globset",
        "blake3",
        "clap",
        "thiserror",
        "anyhow",
        "tracing",
        "tempfile",
        // Reviewed OS shim already present through tempfile; `fs` supports
        // atomic exchange and `process` stays owner-guarded, including the
        // orchestrator's read-only stage euid lookup.
        "rustix",
        // Reviewed hash impl for the pre-seed manifest writer (SHA-256 of the
        // fresh helper) and generator identity comparison; pure Rust, default
        // features only, so pin comparison uses the audited implementation.
        "sha2",
        // HCL structural parser for tofu (T10, S8); hcl-rs 0.19.8 is
        // compile-gated at MSRV 1.98.1, default features only, with
        // facade-owned byte/count/depth caps and no expression evaluation.
        "hcl",
        // Reviewed proc-macro token tree for the test-source scanner.
        "proc-macro2",
        // Reviewed Rust AST (`full`, `visit`) for the source closure guard.
        "syn",
        "flate2",
        // Bounded GitHub receipt ZIP reader; only pure-Rust deflate decoding
        // is enabled, so artifacts are validated without extracting paths.
        "zip",
        "rustls",
        "rustls-native-certs",
        "ureq",
        // CLI trailer compatibility preserves Python Unicode word-boundary semantics.
        "unicode-general-category",
    ];
    for (dir, _) in MEMBERS {
        let body = manifest(dir)?;
        assert!(!body.contains("tokio"), "{dir} must not use tokio");
        for line in dep_lines(&body) {
            let key = dep_key(line);
            if key.starts_with("velnor-actions") {
                continue;
            }
            let archive_decoder = reviewed_archive_dependency(dir, key, line);
            assert!(
                allowed.contains(&key) || archive_decoder,
                "{dir} uses {key}"
            );
            if let Some(index) = line.find("features").filter(|_| !archive_decoder) {
                let quoted: Vec<&str> = line[index..].split('"').collect();
                for feature in quoted.into_iter().skip(1).step_by(2) {
                    assert!(
                        allowed_feature(dir, key, feature),
                        "{dir}/{key} feature {feature}"
                    );
                }
            }
            let feature_backend =
                key == "flate2" && body.contains("zip = ") && body.contains("deflate-flate2");
            assert!(
                dep_referenced(dir, &key.replace('-', "_"))? || feature_backend,
                "{dir} never uses {key}"
            );
        }
    }
    Ok(())
}

fn allowed_feature(dir: &str, key: &str, feature: &str) -> bool {
    feature == "derive"
        || (key == "rustix" && feature == "fs")
        || (key == "rustix"
            && feature == "process"
            && matches!(
                dir,
                "crates/velnor-actions-freshness"
                    | "crates/velnor-actions-mise"
                    | "crates/velnor-actions-orchestrator"
            ))
        || (dir == "crates/velnor-actions-freshness"
            && ((key == "flate2" && feature == "rust_backend")
                || (key == "rustls" && feature == "ring")
                || (key == "syn" && ["full", "parsing"].contains(&feature))
                || (key == "ureq" && feature == "rustls-no-provider")))
        || (dir == "crates/velnor-actions-cli"
            && key == "syn"
            && matches!(feature, "full" | "visit"))
        // F2A scanner tests (dev-deps): syn parse + span lines.
        || (dir == "crates/velnor-actions-orchestrator"
            && ((key == "syn" && matches!(feature, "full" | "parsing" | "printing" | "visit"))
                || (key == "proc-macro2" && feature == "span-locations")))
}
