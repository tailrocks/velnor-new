//! Repo-shape policy continued: dependencies, tests, sizes, CLI structure.

#[path = "impl_repo_git_policy.rs"]
mod git_policy;

use std::collections::BTreeMap;
use std::error::Error;

use crate::impl_repo_policy::{
    MEMBERS, dep_key, dep_lines, dep_referenced, manifest, p11_toml, read, tree_files,
};

#[path = "impl_repo_deps_layout.rs"]
mod layout;

/// Intra-workspace edges allowed per member package.
fn expected_internal(dir: &str) -> Vec<&str> {
    match dir {
        "crates/velnor-actions-contract" => vec![],
        "crates/velnor-actions-orchestrator" => vec![
            "velnor-actions-actionlint",
            "velnor-actions-contract",
            "velnor-actions-mise",
            "velnor-actions-rust",
            "velnor-actions-tofu",
            "velnor-actions-workflow-renderer",
        ],
        "crates/velnor-actions-cli" => vec!["velnor-actions-orchestrator"],
        _ => vec!["velnor-actions-contract"],
    }
}

const ALLOWED_EXTERNAL_DEPS: &[&str] = &[
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
    // Reviewed OS shim for the P09 atomic directory exchange; already
    // in the lockfile via tempfile, zero new crates.
    "rustix",
    // Reviewed hash impl for the pre-seed manifest writer (SHA-256 of
    // the fresh helper) and generator SHA-256 identity (replaces
    // hand-rolled SHA-256 so release-pin comparison cannot drift from
    // the audited implementation); pure Rust, default features only.
    "sha2",
    // Test-only Rust scanners parse paths/imports; runtime adapters never
    // depend on or invoke these analyzers. Syn 3 needs `printing` for spans.
    "proc-macro2",
    "syn",
    // CLI trailer compatibility preserves Python Unicode word-boundary semantics.
    "unicode-general-category",
    // Reviewed HCL structural parser for the tofu stack (T10, S8):
    // `hcl` renames `hcl-rs` 0.19.8 (Q1 pre-qualified; MSRV
    // compile-gated at 1.98.1); default features only, facade-owned
    // byte/count/depth caps, no expression evaluation.
    "hcl",
    // P12 baseline archives admit one bounded ZIP entry; raw DEFLATE
    // must consume its full input. Both paths use only the zlib-rs backend.
    "flate2",
    "zip",
];

fn assert_test_scanner_is_test_only(
    dir: &str,
    body: &str,
    key: &str,
) -> Result<(), Box<dyn Error>> {
    if key != "syn" && key != "proc-macro2" {
        return Ok(());
    }
    let allowed_owner = if key == "syn" {
        [
            "crates/velnor-actions-native",
            "crates/velnor-actions-orchestrator",
        ]
        .contains(&dir)
    } else {
        dir == "crates/velnor-actions-orchestrator"
    };
    assert!(allowed_owner, "{dir} may use test-only {key}");
    let doc = p11_toml::parse(body)?;
    assert!(
        p11_toml::section(&doc, "dependencies")
            .is_none_or(|deps| deps.pairs.iter().all(|pair| pair.0 != key)),
        "{key} is test-only"
    );
    Ok(())
}

fn assert_narrow_features(dir: &str, key: &str, line: &str) {
    if let Some(index) = line.find("features") {
        let quoted: Vec<&str> = line[index..].split('"').collect();
        for feature in quoted.into_iter().skip(1).step_by(2) {
            // Only `derive` globally, plus `fs` on rustix for the P09 atomic
            // directory exchange (no net/pty/terminal).
            let narrow = feature == "derive"
                || (key == "rustix" && feature == "fs")
                || (key == "syn" && ["full", "parsing", "printing", "visit"].contains(&feature))
                || (key == "proc-macro2" && feature == "span-locations")
                || (key == "flate2" && feature == "zlib-rs")
                || (key == "zip" && feature == "deflate-flate2-zlib-rs");
            assert!(narrow, "{dir}/{key} feature {feature}");
        }
    }
}

fn assert_archive_dependency_is_narrow(
    dir: &str,
    body: &str,
    key: &str,
    line: &str,
) -> Result<(), Box<dyn Error>> {
    if key != "flate2" && key != "zip" {
        return Ok(());
    }
    assert_eq!(
        dir, "crates/velnor-actions-orchestrator",
        "{key} is orchestrator-only"
    );
    let doc = p11_toml::parse(body)?;
    assert!(
        p11_toml::section(&doc, "dependencies")
            .is_some_and(|dependencies| { dependencies.pairs.iter().any(|pair| pair.0 == key) }),
        "{dir}/{key} must be a runtime dependency"
    );
    let feature = if key == "flate2" {
        "zlib-rs"
    } else {
        "deflate-flate2-zlib-rs"
    };
    let selected_features: Vec<&str> = line
        .find("features")
        .map(|index| line[index..].split('"').skip(1).step_by(2).collect())
        .unwrap_or_default();
    assert!(
        line.contains("default-features = false") && selected_features == [feature],
        "{dir}/{key} must disable defaults and select only {feature}"
    );
    Ok(())
}

fn assert_external_dependency_is_narrow(
    dir: &str,
    body: &str,
    line: &str,
) -> Result<(), Box<dyn Error>> {
    let key = dep_key(line);
    if key.starts_with("velnor-actions") {
        return Ok(());
    }
    assert!(ALLOWED_EXTERNAL_DEPS.contains(&key), "{dir} uses {key}");
    assert_test_scanner_is_test_only(dir, body, key)?;
    assert_narrow_features(dir, key, line);
    assert_archive_dependency_is_narrow(dir, body, key, line)?;
    let import_name = match key {
        "proc-macro2" => "proc_macro2",
        "unicode-general-category" => "unicode_general_category",
        _ => key,
    };
    assert!(dep_referenced(dir, import_name)?, "{dir} never uses {key}");
    Ok(())
}

#[test]
fn dependency_edges_match_ownership_table() -> Result<(), Box<dyn Error>> {
    for (dir, _) in MEMBERS {
        let doc = p11_toml::parse(&manifest(dir)?)?;
        let mut found: Vec<String> = p11_toml::section(&doc, "dependencies")
            .map(|deps| {
                deps.pairs
                    .iter()
                    .filter(|pair| pair.0.starts_with("velnor-actions"))
                    .map(|pair| pair.0.clone())
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        let mut want: Vec<String> = expected_internal(dir)
            .into_iter()
            .map(str::to_owned)
            .collect();
        want.sort();
        assert_eq!(found, want, "{dir} edges drift");
    }
    Ok(())
}

#[test]
fn external_deps_allowlisted_used_and_narrow() -> Result<(), Box<dyn Error>> {
    for (dir, _) in MEMBERS {
        let body = manifest(dir)?;
        assert!(!body.contains("tokio"), "{dir} must not use tokio");
        for line in dep_lines(&body) {
            assert_external_dependency_is_narrow(dir, &body, line)?;
        }
    }
    Ok(())
}

#[test]
fn dep_versions_exact_and_consistent() -> Result<(), Box<dyn Error>> {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for (dir, _) in MEMBERS {
        for line in dep_lines(&manifest(dir)?) {
            if !line.contains("version") {
                continue;
            }
            let version = line.split('"').nth(1).ok_or("version shape")?;
            assert!(version.starts_with('='), "{dir}: {line} must pin exactly");
            let key = dep_key(line).to_owned();
            if let Some(prior) = seen.insert(key.clone(), version.to_owned()) {
                assert_eq!(prior, version, "{key} version diverges");
            }
        }
    }
    assert!(!seen.is_empty());
    Ok(())
}

#[test]
fn no_custom_linter_modules() -> Result<(), Box<dyn Error>> {
    for (dir, _) in MEMBERS {
        for path in tree_files(&format!("{dir}/src"), "rs")? {
            let name = path
                .file_name()
                .and_then(|stem| stem.to_str())
                .unwrap_or("");
            if !dir.ends_with("actionlint") {
                assert!(!name.contains("linter"), "{}", path.display());
            }
            assert!(
                !std::fs::read_to_string(&path)?.contains("mod linter"),
                "{}",
                path.display()
            );
        }
    }
    Ok(())
}

#[test]
fn cli_tests_assert_through_binary_only() -> Result<(), Box<dyn Error>> {
    // Built at runtime so this very file does not trip its own scan.
    let stem = ["velnor", "actions"].join("_");
    let import = format!("use {stem}");
    let path_use = format!("{stem}::");
    for path in tree_files("crates/velnor-actions-cli/tests", "rs")? {
        let body = std::fs::read_to_string(&path)?;
        assert!(
            !body.contains(&import),
            "{} imports implementation",
            path.display()
        );
        assert!(
            !body.contains(&path_use),
            "{} uses implementation",
            path.display()
        );
    }
    Ok(())
}

/// Physical lines: newline count, matching `wc -l`.
pub(crate) fn physical_lines(body: &str) -> usize {
    body.bytes().filter(|byte| *byte == b'\n').count()
}

#[test]
fn cli_invokes_no_tools_directly() -> Result<(), Box<dyn Error>> {
    let banned = [
        "process::Command",
        "Command::new",
        ".spawn(",
        ".status()",
        ".output()",
        "cargo",
        "mise",
        "mbx",
        "nextest",
        "rustup",
        "\"gh\"",
        "shell",
        "Shell",
        "sh -c",
    ];
    for path in tree_files("crates/velnor-actions-cli/src", "rs")? {
        let body = std::fs::read_to_string(&path)?;
        for token in banned {
            assert!(!body.contains(token), "{} leaks {token}", path.display());
        }
        for (index, _) in body.match_indices("std::process::") {
            let rest = &body[index + "std::process::".len()..];
            assert!(
                rest.starts_with("ExitCode"),
                "{} uses {rest:?}",
                path.display()
            );
        }
    }
    Ok(())
}

#[test]
fn init_reads_no_tool_files() -> Result<(), Box<dyn Error>> {
    let body = read("crates/velnor-actions-orchestrator/src/init.rs")?;
    for token in [
        "read_to_string",
        "File::open",
        ".read(",
        "read_dir",
        "read_link",
        "io::Read",
    ] {
        assert!(!body.contains(token), "init.rs inspects via {token}");
    }
    Ok(())
}

#[test]
fn parse_tests_live_outside_src() -> Result<(), Box<dyn Error>> {
    for path in tree_files("crates/velnor-actions-cli/src", "rs")? {
        assert!(
            !std::fs::read_to_string(&path)?.contains("#[test]"),
            "{}",
            path.display()
        );
    }
    assert!(read("crates/velnor-actions-cli/tests/impl_cli_args.rs")?.contains("#[test]"));
    Ok(())
}
