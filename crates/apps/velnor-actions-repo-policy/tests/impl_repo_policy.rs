//! Repo-shape policy: workspace, manifests, lints, versions, sizes.
//! Reads repo files via `CARGO_MANIFEST_DIR`; pins must-clauses for streams.

use std::error::Error;
use std::path::{Path, PathBuf};

#[path = "alint_miniyaml.rs"]
pub(crate) mod alint_miniyaml;
#[path = "fixtures/p11_alint.rs"]
pub(crate) mod p11_alint;
#[path = "fixtures/p11_compiler.rs"]
mod p11_compiler;
#[path = "fixtures/p11_metadata.rs"]
mod p11_metadata;
#[path = "fixtures/p11_toml.rs"]
pub(crate) mod p11_toml;
#[path = "fixtures/p12_harness.rs"]
mod p12_harness;
#[path = "fixtures/p12_live.rs"]
mod p12_live;
#[path = "fixtures/p12_manifest.rs"]
mod p12_manifest;
#[path = "fixtures/p12_mutants.rs"]
mod p12_mutants;
#[path = "fixtures/p12_policy.rs"]
mod p12_policy;
#[path = "fixtures/p12_policy_b.rs"]
mod p12_policy_b;
#[path = "fixtures/p12_upstream.rs"]
mod p12_upstream;

/// Expected member directories (package name is the leaf).
pub(crate) const MEMBERS: [&str; 20] = [
    "crates/adapters/velnor-actions-actionlint",
    "crates/apps/velnor-actions-cli",
    "crates/apps/velnor-actions-repo-policy",
    "crates/core/velnor-actions-contract",
    "crates/core/velnor-actions-contract-config",
    "crates/core/velnor-actions-contract-planning",
    "crates/core/velnor-actions-contract-release",
    "crates/core/velnor-actions-contract-workflow",
    "crates/adapters/velnor-actions-mise",
    "crates/adapters/velnor-actions-mise-cache",
    "crates/adapters/velnor-actions-mise-catalog",
    "crates/adapters/velnor-actions-mise-core",
    "crates/adapters/velnor-actions-mise-nextest",
    "crates/adapters/velnor-actions-mise-probes",
    "crates/services/velnor-actions-orchestrator",
    "crates/adapters/velnor-actions-rust",
    "crates/adapters/velnor-actions-rust-core",
    "crates/adapters/velnor-actions-tofu",
    "crates/adapters/velnor-actions-tofu-core",
    "crates/services/velnor-actions-workflow-renderer",
];

/// Package name for a member dir (the leaf segment).
pub(crate) fn package(dir: &str) -> &str {
    dir.rsplit('/').next().unwrap_or("")
}

pub(crate) const WORKSPACE_ROOTS: [&str; 2] = ["", "crates/velnor-runner"];

/// Repo root: two levels above this crate's manifest directory.
pub(crate) fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// Read a repo-relative file to a string.
pub(crate) fn read(relative: &str) -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(repo_root().join(relative))?)
}

/// Read one member manifest.
pub(crate) fn manifest(dir: &str) -> Result<String, Box<dyn Error>> {
    read(&format!("{dir}/Cargo.toml"))
}

/// Lines in one exact TOML table, excluding comments and the table header.
fn manifest_section<'a>(body: &'a str, section: &str) -> Vec<&'a str> {
    let header = format!("[{section}]");
    let mut active = false;
    let mut lines = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            active = line == header;
        } else if active && !line.is_empty() && !line.starts_with('#') {
            lines.push(line);
        }
    }
    lines
}

/// Files with `extension` under a repo-relative dir, recursively.
pub(crate) fn tree_files(relative: &str, extension: &str) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut out = Vec::new();
    let mut pending = vec![repo_root().join(relative)];
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            } else if entry.path().extension().is_some_and(|ext| ext == extension) {
                out.push(entry.path());
            }
        }
    }
    out.sort();
    Ok(out)
}

/// First double-quoted value on the first line containing `key`.
pub(crate) fn quoted_value(text: &str, key: &str) -> Result<String, Box<dyn Error>> {
    text.lines()
        .filter(|line| line.contains(key))
        .filter_map(|line| line.split('"').nth(1))
        .map(str::to_owned)
        .next()
        .ok_or_else(|| format!("{key} not found").into())
}

/// Non-comment lines inside dependency sections of a manifest.
pub(crate) fn dep_lines(manifest: &str) -> Vec<&str> {
    let mut in_deps = false;
    let mut out = Vec::new();
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_deps = matches!(
                trimmed,
                "[dependencies]" | "[dev-dependencies]" | "[build-dependencies]"
            );
        } else if in_deps && !trimmed.is_empty() && !trimmed.starts_with('#') {
            out.push(trimmed);
        }
    }
    out
}

/// Dependency key before `=` on a manifest dep line.
pub(crate) fn dep_key(line: &str) -> &str {
    line.split('=').next().unwrap_or(line).trim()
}

/// Count `#[test]` markers across a member's test files.
pub(crate) fn test_markers(dir: &str) -> Result<usize, Box<dyn Error>> {
    let mut count = 0;
    for path in tree_files(&format!("{dir}/tests"), "rs")? {
        count += std::fs::read_to_string(&path)?.matches("#[test]").count();
    }
    Ok(count)
}

/// True when `dep` is referenced from `dir` sources or tests.
pub(crate) fn dep_referenced(dir: &str, dep: &str) -> Result<bool, Box<dyn Error>> {
    let crate_name = dep.replace('-', "_");
    let import = format!("use {crate_name}");
    let path = format!("{crate_name}::");
    for area in ["src", "tests"] {
        for file in tree_files(&format!("{dir}/{area}"), "rs")? {
            let body = std::fs::read_to_string(&file)?;
            if body.contains(&import) || body.contains(&path) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[test]
fn workspace_lists_members_runner_and_archive_guard() -> Result<(), Box<dyn Error>> {
    let root = read("Cargo.toml")?;
    let start = root.find("members = [").ok_or("members block")?;
    let block = root[start..].split(']').next().ok_or("members end")?;
    let runner = p11_metadata::RUNNER_MEMBERS.len();
    assert_eq!(
        block.matches("crates/").count(),
        MEMBERS.len() + runner + 1,
        "{block}"
    );
    for dir in MEMBERS {
        assert!(block.contains(&format!("\"{dir}\"")), "{dir} not listed");
    }
    assert!(
        block.contains("\"crates/tools/velnor-archive-guard\""),
        "archive guard workspace member missing"
    );
    Ok(())
}

#[test]
fn root_manifest_is_virtual_with_explicit_members() -> Result<(), Box<dyn Error>> {
    let root = read("Cargo.toml")?;
    assert!(root.contains("[workspace]"));
    assert!(!root.contains("[package]"), "root must stay virtual");
    for dir in MEMBERS {
        assert!(root.contains(dir), "{dir} not explicit");
    }
    assert!(!root.contains("crates/*"), "members must not glob");
    Ok(())
}

#[test]
fn package_names_use_purpose_suffix() -> Result<(), Box<dyn Error>> {
    for dir in MEMBERS {
        let package = package(dir);
        let body = manifest(dir)?;
        assert!(body.contains(&format!("name = \"{package}\"")), "{dir}");
        let purpose = package.strip_prefix("velnor-actions-").ok_or(package)?;
        assert!(!purpose.is_empty(), "{dir}");
    }
    Ok(())
}

#[test]
fn generic_names_forbidden() -> Result<(), Box<dyn Error>> {
    let stems = ["model", "core", "common", "utils", "util"];
    for dir in MEMBERS {
        let body = manifest(dir)?;
        for prefix in ["velnor-", "velnor-actions-"] {
            for stem in stems {
                let name = format!("{prefix}{stem}");
                assert!(!body.contains(&name), "{dir} uses {name}");
            }
        }
        assert!(!body.contains("velnor-rust"), "{dir} uses velnor-rust");
    }
    Ok(())
}

#[test]
fn velnor_name_never_published() -> Result<(), Box<dyn Error>> {
    let mut bins = 0;
    for dir in MEMBERS {
        let body = manifest(dir)?;
        assert!(!body.contains("name = \"velnor\""), "{dir}");
        assert!(!body.contains("[alias"), "{dir}");
        bins += body.matches("[[bin]]").count();
    }
    assert_eq!(bins, 1, "sole binary must be velnor-actions");
    let cli = manifest("crates/apps/velnor-actions-cli")?;
    assert!(cli.contains("name = \"velnor-actions\""));
    if let Ok(cargo_config) = read(".cargo/config.toml") {
        assert!(!cargo_config.contains("[alias]"));
    }
    Ok(())
}

#[test]
fn toolchain_edition_and_resolver() -> Result<(), Box<dyn Error>> {
    for workspace in WORKSPACE_ROOTS {
        let path = format!("{workspace}/Cargo.toml");
        let path = path.trim_start_matches('/');
        let root = read(path)?;
        assert!(root.contains("edition = \"2024\""), "{path}");
        assert!(root.contains("resolver = \"3\""), "{path}");
        assert!(root.contains("rust-version = \"1.98\""), "{path}");
    }
    Ok(())
}

/// First two dot-separated version components.
pub(crate) fn minor(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

#[test]
fn rust_version_tracks_toolchain() -> Result<(), Box<dyn Error>> {
    let catalog_src = read("crates/adapters/velnor-actions-mise-catalog/src/catalog.rs")?;
    let catalog = quoted_value(&catalog_src, "RUST_VERSION")?;
    let mise = quoted_value(&read("mise.toml")?, "rust = ")?;
    for workspace_root in WORKSPACE_ROOTS {
        let path = format!("{workspace_root}/Cargo.toml");
        let path = path.trim_start_matches('/');
        let workspace = quoted_value(&read(path)?, "rust-version")?;
        assert_eq!(minor(&workspace), minor(&catalog), "{path} catalog drift");
        assert_eq!(minor(&workspace), minor(&mise), "{path} mise drift");
    }
    Ok(())
}

/// `rust-toolchain.toml` is hand-maintained (Velnor never modifies it) and
/// MUST track the Mise-authoritative Rust version at semver-minor.
#[test]
fn toolchain_file_tracks_mise() -> Result<(), Box<dyn Error>> {
    let channel = quoted_value(&read("rust-toolchain.toml")?, "channel")?;
    let mise = quoted_value(&read("mise.toml")?, "rust = ")?;
    assert_eq!(channel, "1.98.1", "toolchain channel drift");
    assert_eq!(minor(&channel), minor(&mise), "mise drift");
    Ok(())
}

#[test]
fn members_inherit_workspace_settings() -> Result<(), Box<dyn Error>> {
    for (dir, _) in MEMBERS
        .into_iter()
        .map(|member| (member, member))
        .chain(p11_metadata::RUNNER_MEMBERS)
        .chain([("crates/tools/velnor-archive-guard", "velnor-archive-guard")])
    {
        let body = manifest(dir)?;
        let package = manifest_section(&body, "package");
        assert!(
            package.contains(&"edition.workspace = true"),
            "{dir} must inherit edition"
        );
        assert!(
            package.contains(&"rust-version.workspace = true"),
            "{dir} must inherit rust-version"
        );
        assert!(
            !package.iter().any(|line| line.starts_with("edition =")),
            "{dir} must not override inherited edition"
        );
        assert!(
            !package
                .iter()
                .any(|line| line.starts_with("rust-version =")),
            "{dir} must not override inherited rust-version"
        );
        assert_eq!(
            manifest_section(&body, "lints"),
            ["workspace = true"],
            "{dir} must inherit the workspace lint baseline purely"
        );
    }
    Ok(())
}

#[test]
fn workspace_lints_match_baseline() -> Result<(), Box<dyn Error>> {
    let baseline = [
        "unsafe_code = \"forbid\"",
        "unused_must_use = \"deny\"",
        "unexpected_cfgs = \"deny\"",
        "unfulfilled_lint_expectations = \"deny\"",
        "missing_docs = \"warn\"",
        "missing_debug_implementations = \"warn\"",
        "unreachable_pub = \"warn\"",
        "rust_2018_idioms = { level = \"warn\", priority = -1 }",
        "all = { level = \"warn\", priority = -1 }",
        "pedantic = { level = \"warn\", priority = -1 }",
        "too_many_lines = \"deny\"",
        "unwrap_used = \"deny\"",
        "expect_used = \"deny\"",
        "panic = \"deny\"",
        "todo = \"deny\"",
        "unimplemented = \"deny\"",
        "dbg_macro = \"deny\"",
        "mem_forget = \"deny\"",
        "await_holding_lock = \"deny\"",
        "await_holding_refcell_ref = \"deny\"",
        "let_underscore_future = \"deny\"",
        "let_underscore_must_use = \"deny\"",
        "undocumented_unsafe_blocks = \"deny\"",
        "allow_attributes_without_reason = \"deny\"",
        "allow_attributes = \"warn\"",
        "broken_intra_doc_links = \"deny\"",
        "private_intra_doc_links = \"deny\"",
    ];
    for workspace_root in WORKSPACE_ROOTS {
        let path = if workspace_root.is_empty() {
            "Cargo.toml".to_owned()
        } else {
            format!("{workspace_root}/Cargo.toml")
        };
        let root = read(&path)?;
        for line in baseline {
            assert!(root.contains(line), "{path} baseline misses {line}");
        }
    }
    Ok(())
}

#[test]
fn clippy_toml_has_five_settings() -> Result<(), Box<dyn Error>> {
    let body = read("clippy.toml")?;
    let settings = [
        "too-many-lines-threshold = 80",
        "allow-unwrap-in-tests = false",
        "allow-expect-in-tests = true",
        "allow-panic-in-tests = true",
        "check-incompatible-msrv-in-tests = true",
    ];
    for line in settings {
        assert!(body.contains(line), "clippy.toml misses {line}");
    }
    let live = body
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
        .count();
    assert_eq!(live, 5, "clippy.toml must hold exactly 5 settings");
    Ok(())
}

// Behavioral enforcement (forbidden groups, alint verdicts, unsafe
// absence) and P11 strictness guarantees live in the sibling
// `impl_repo_strictness` module, which shares the helpers above.
