//! Repo-shape policy: workspace, manifests, lints, versions, sizes.
//! Reads repo files via `CARGO_MANIFEST_DIR`; pins must-clauses for streams.

use std::error::Error;
use std::path::{Path, PathBuf};

#[path = "alint_miniyaml.rs"]
mod alint_miniyaml;
#[path = "fixtures/p11_alint.rs"]
mod p11_alint;
#[path = "fixtures/p11_compiler.rs"]
mod p11_compiler;
#[path = "fixtures/p11_metadata.rs"]
mod p11_metadata;
#[path = "fixtures/p11_toml.rs"]
mod p11_toml;
#[path = "fixtures/p12_harness.rs"]
mod p12_harness;
#[path = "fixtures/p12_live.rs"]
mod p12_live;
#[path = "fixtures/p12_manifest.rs"]
mod p12_manifest;
#[path = "fixtures/p12_policy.rs"]
mod p12_policy;

/// Expected members as (directory, package name).
pub(crate) const MEMBERS: [(&str, &str); 7] = [
    ("crates/velnor-actions-actionlint", "velnor-actions-actionlint"),
    ("crates/velnor-actions-cli", "velnor-actions-cli"),
    ("crates/velnor-actions-contract", "velnor-actions-contract"),
    ("crates/velnor-actions-mise", "velnor-actions-mise"),
    ("crates/velnor-actions-orchestrator", "velnor-actions-orchestrator"),
    ("crates/velnor-actions-rust", "velnor-actions-rust"),
    ("crates/velnor-actions-workflow-renderer", "velnor-actions-workflow-renderer"),
];

/// Repo root: two levels above this crate's manifest directory.
pub(crate) fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Read a repo-relative file to a string.
pub(crate) fn read(relative: &str) -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(repo_root().join(relative))?)
}

/// Read one member manifest.
pub(crate) fn manifest(dir: &str) -> Result<String, Box<dyn Error>> {
    read(&format!("{dir}/Cargo.toml"))
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
    let import = format!("use {dep}");
    let path = format!("{dep}::");
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
fn workspace_lists_exactly_seven_members() -> Result<(), Box<dyn Error>> {
    let root = read("Cargo.toml")?;
    let start = root.find("members = [").ok_or("members block")?;
    let block = root[start..].split(']').next().ok_or("members end")?;
    assert_eq!(block.matches("crates/").count(), 7, "{block}");
    for (dir, _) in MEMBERS {
        assert!(block.contains(&format!("\"{dir}\"")), "{dir} not listed");
    }
    Ok(())
}

#[test]
fn root_manifest_is_virtual_with_explicit_members() -> Result<(), Box<dyn Error>> {
    let root = read("Cargo.toml")?;
    assert!(root.contains("[workspace]"));
    assert!(!root.contains("[package]"), "root must stay virtual");
    for (dir, _) in MEMBERS {
        assert!(root.contains(dir), "{dir} not explicit");
    }
    assert!(!root.contains("crates/*"), "members must not glob");
    Ok(())
}

#[test]
fn package_names_use_purpose_suffix() -> Result<(), Box<dyn Error>> {
    for (dir, package) in MEMBERS {
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
    for (dir, _) in MEMBERS {
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
    for (dir, _) in MEMBERS {
        let body = manifest(dir)?;
        assert!(!body.contains("name = \"velnor\""), "{dir}");
        assert!(!body.contains("[alias"), "{dir}");
        bins += body.matches("[[bin]]").count();
    }
    assert_eq!(bins, 1, "sole binary must be velnor-actions");
    let cli = manifest("crates/velnor-actions-cli")?;
    assert!(cli.contains("name = \"velnor-actions\""));
    if let Ok(cargo_config) = read(".cargo/config.toml") {
        assert!(!cargo_config.contains("[alias]"));
    }
    Ok(())
}

#[test]
fn toolchain_edition_and_resolver() -> Result<(), Box<dyn Error>> {
    let root = read("Cargo.toml")?;
    assert!(root.contains("edition = \"2024\""));
    assert!(root.contains("resolver = \"3\""));
    assert!(root.contains("rust-version = "));
    Ok(())
}

/// First two dot-separated version components.
pub(crate) fn minor(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

#[test]
fn rust_version_tracks_toolchain() -> Result<(), Box<dyn Error>> {
    let workspace = quoted_value(&read("Cargo.toml")?, "rust-version")?;
    let catalog_src = read("crates/velnor-actions-mise/src/catalog.rs")?;
    let catalog = quoted_value(&catalog_src, "RUST_VERSION")?;
    let mise = quoted_value(&read("mise.toml")?, "rust = ")?;
    assert_eq!(minor(&workspace), minor(&catalog), "catalog drift");
    assert_eq!(minor(&workspace), minor(&mise), "mise drift");
    Ok(())
}

#[test]
fn members_inherit_workspace_settings() -> Result<(), Box<dyn Error>> {
    for (dir, _) in MEMBERS {
        let body = manifest(dir)?;
        for key in ["edition.workspace = true", "rust-version.workspace = true"] {
            assert!(body.contains(key), "{dir} misses {key}");
        }
        assert!(body.contains("[lints]"), "{dir} misses [lints]");
        assert!(body.contains("workspace = true"), "{dir} misses workspace");
    }
    Ok(())
}

#[test]
fn workspace_lints_match_baseline() -> Result<(), Box<dyn Error>> {
    let root = read("Cargo.toml")?;
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
    for line in baseline {
        assert!(root.contains(line), "baseline misses {line}");
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

#[test]
fn no_restriction_or_nursery_groups() -> Result<(), Box<dyn Error>> {
    for (dir, _) in MEMBERS {
        let body = manifest(dir)?;
        assert!(!body.contains("restriction"), "{dir}");
        assert!(!body.contains("nursery"), "{dir}");
    }
    let root = read("Cargo.toml")?;
    assert!(!root.contains("restriction"));
    assert!(!root.contains("nursery"));
    Ok(())
}

#[test]
fn no_nightly_toolchain() -> Result<(), Box<dyn Error>> {
    for file in [
        "Cargo.toml",
        "mise.toml",
        "crates/velnor-actions-mise/src/catalog.rs",
        ".github/workflows/velnor.yml",
    ] {
        assert!(!read(file)?.to_lowercase().contains("nightly"), "{file}");
    }
    for (dir, _) in MEMBERS {
        assert!(!manifest(dir)?.to_lowercase().contains("nightly"), "{dir}");
    }
    Ok(())
}

#[test]
fn no_git_dependencies() -> Result<(), Box<dyn Error>> {
    for (dir, _) in MEMBERS {
        let body = manifest(dir)?;
        assert!(!body.contains("git="), "{dir}");
        assert!(!body.contains("git ="), "{dir}");
    }
    let lock = read("Cargo.lock")?;
    assert!(!lock.contains("git+"), "lockfile has git source");
    Ok(())
}

#[test]
fn alint_config_semantic_policy() -> Result<(), Box<dyn Error>> {
    p11_alint::check_extended_policy(&read(".alint.yml")?)
}

#[test]
fn alint_rule_fixtures_pass_fail_and_express_command() -> Result<(), Box<dyn Error>> {
    for row in &alint_miniyaml::EXPECTED {
        let id = row.id;
        let pass = alint_miniyaml::parse(&alint_miniyaml::fixture(id, "pass")?)?;
        alint_miniyaml::check_policy(&pass).map_err(|err| format!("{id} pass: {err}"))?;
        let fail = alint_miniyaml::parse(&alint_miniyaml::fixture(id, "fail")?)?;
        let failed = alint_miniyaml::check_policy(&fail).is_err();
        assert!(failed, "{id} fail passed");
    }
    let extra = alint_miniyaml::parse(&alint_miniyaml::fixture("command", "expressible")?)?;
    let added = alint_miniyaml::rule(&extra, "example-toml-edition-rule").ok_or("added rule")?;
    alint_miniyaml::check_rule_shape(added)?;
    let rejected = alint_miniyaml::check_policy(&extra).is_err();
    assert!(rejected, "unknown id passed");
    Ok(())
}

#[test]
fn alint_comment_only_edits_keep_verdicts() -> Result<(), Box<dyn Error>> {
    let live = read(".alint.yml")?;
    assert!(live.contains("\nversion: 1\n"), "anchor for inline probes");
    let inline = live.replacen("\nversion: 1\n", "\n# probe\nversion: 1 # probe\n# probe\n", 1);
    for mutated in [format!("# probe\n{live}"), format!("{live}\n# probe\n"), inline] {
        p11_alint::check_extended_policy(&mutated)?;
    }
    for row in &alint_miniyaml::EXPECTED {
        let wrapped = format!("# probe\n{}", alint_miniyaml::fixture(row.id, "pass")?);
        let parsed = alint_miniyaml::parse(&wrapped)?;
        alint_miniyaml::check_policy(&parsed).map_err(|err| format!("{}: {err}", row.id))?;
    }
    Ok(())
}

#[test]
fn unsafe_forbidden_and_absent() -> Result<(), Box<dyn Error>> {
    assert!(read("Cargo.toml")?.contains("unsafe_code = \"forbid\""));
    let spellings = [
        "unsafe {",
        "unsafe{",
        "unsafe fn",
        "unsafe impl",
        "unsafe trait",
        "unsafe extern",
        "#[unsafe",
    ];
    for (dir, _) in MEMBERS {
        for path in tree_files(&format!("{dir}/src"), "rs")? {
            let body = std::fs::read_to_string(&path)?;
            for spelling in spellings {
                assert!(
                    !body.contains(spelling),
                    "{} has {spelling}",
                    path.display()
                );
            }
        }
    }
    Ok(())
}
