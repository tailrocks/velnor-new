//! Repo-shape pins: workspace layout, alint scoping, registry, format.
//!
//! Covers ARCH-1.10, ARCH-1.12, ARCH-1.5, RQ-1.1, RQ-4.1, RQ-4.2, RQ-5.2,
//! RQ-5.3, RQ-6.6. Reads repository files only; asserts through content.

use std::error::Error;
use std::path::{Path, PathBuf};

use crate::impl_repo_policy::{MEMBERS, read, repo_root};

/// Every non-directory entry under `dir`, symlink-safe (never follows links).
fn collect_entries(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // Product content is never hidden: skip build output, VCS state,
            // and ignored tool checkouts (nested worktrees carry their own
            // crates/*/Cargo.toml package roots that are not repo content).
            if name == "target" || name.starts_with('.') {
                continue;
            }
            collect_entries(&entry.path(), out)?;
        } else if file_type.is_file() {
            out.push(entry.path());
        }
    }
    Ok(())
}

#[test]
fn arch110_product_roots_live_under_crates() -> Result<(), Box<dyn Error>> {
    for dir in MEMBERS {
        assert!(dir.starts_with("crates/"), "{dir} escapes crates/");
    }
    let root = read("Cargo.toml")?;
    let start = root.find("members = [").ok_or("members block")?;
    let block = root[start..].split(']').next().ok_or("members end")?;
    for segment in block.split('"').skip(1).step_by(2) {
        assert!(segment.starts_with("crates/"), "{segment} escapes crates/");
    }
    let repo = repo_root();
    let mut entries = Vec::new();
    collect_entries(&repo, &mut entries)?;
    for path in entries {
        let relative = path.strip_prefix(&repo)?;
        let name = path
            .file_name()
            .and_then(|stem| stem.to_str())
            .unwrap_or("");
        if path.extension().is_some_and(|ext| ext == "rs") {
            assert!(
                relative.starts_with("crates") || relative.starts_with("fixtures"),
                "{} escapes crates/ and fixtures/",
                relative.display()
            );
        }
        if name == "Cargo.toml" && std::fs::read_to_string(&path)?.contains("[package]") {
            assert!(
                relative.starts_with("crates") || relative.starts_with("fixtures"),
                "{} is a package root outside crates/",
                relative.display()
            );
        }
    }
    Ok(())
}

#[test]
fn arch112_alint_scopes_product_paths() -> Result<(), Box<dyn Error>> {
    let config = read(".alint.yml")?;
    assert!(
        config.contains("ignore:"),
        "alint must declare an ignore block"
    );
    assert!(config.contains("fixtures/**"), "fixtures must be ignored");
    for include in [
        "crates/velnor-actions-*/src/**/*.rs",
        "crates/velnor-actions-*/tests/**/*.rs",
        "crates/velnor-archive-guard/src/**/*.rs",
        "crates/velnor-archive-guard/tests/**/*.rs",
    ] {
        assert!(
            config.contains(include),
            "alint misses handwritten Rust source {include}"
        );
    }
    for include in [
        "crates/velnor-archive-guard/src/lib.rs",
        "crates/velnor-archive-guard/src/main.rs",
    ] {
        assert!(config.contains(include), "alint misses helper {include}");
    }
    for exclude in ["**/fixtures/**", "**/testdata/**"] {
        assert!(
            config.contains(exclude),
            "alint misses test-input {exclude}"
        );
    }
    Ok(())
}

#[test]
fn arch15_all_stacks_explicit() -> Result<(), Box<dyn Error>> {
    for (dir, id) in [
        ("crates/adapters/velnor-actions-rust", "rust"),
        ("crates/adapters/velnor-actions-tofu-core", "tofu"),
    ] {
        let detect = read(&format!("{dir}/src/detect.rs"))?;
        assert!(
            !detect.contains("const REGISTERED_STACKS"),
            "contract owns the single stack registry; no {id} mirror"
        );
        assert!(
            detect.contains("VelnorConfig::REGISTERED_STACKS"),
            "detector self-check must read the contract registry ({id})"
        );
        let lib = read(&format!("{dir}/src/lib.rs"))?;
        assert!(
            lib.contains(&format!("STACK_ID: &str = \"{id}\"")),
            "{id} crate must name its stack explicitly"
        );
        assert!(
            detect.contains("crate::STACK_ID"),
            "detector records must carry the explicit id ({id})"
        );
    }
    Ok(())
}

#[test]
fn rq11_repo_shape_files_present() {
    let files = [
        "Cargo.toml",
        "Cargo.lock",
        "clippy.toml",
        "deny.toml",
        "rustfmt.toml",
        "CODEOWNERS",
        ".alint.yml",
        ".config/nextest.toml",
        "AGENTS.md",
        ".velnor/config.toml",
        ".velnor/version-policy.toml",
    ];
    for file in files {
        assert!(repo_root().join(file).is_file(), "{file} missing");
    }
    for dir in MEMBERS {
        assert!(repo_root().join(dir).is_dir(), "{dir} missing");
    }
    assert!(
        !repo_root().join("mise.lock").exists(),
        "mise.lock must stay absent (RQ-2.12)"
    );
}

#[test]
fn rq41_deviation_and_no_enforcement() -> Result<(), Box<dyn Error>> {
    let deviations = read("docs/content/docs/implemented/deviations.mdx")?;
    assert!(
        deviations.contains("RQ-4.1"),
        "SHOULD deviation must be recorded"
    );
    assert!(
        deviations.contains("tests/impl_*.rs"),
        "deviation must name the chosen layout"
    );
    assert!(
        deviations.contains("MUST-NOT"),
        "deviation must honor the MUST-NOT halves"
    );
    assert!(
        read(".alint.yml")?.contains("test-layout"),
        "alint must disclaim layout enforcement"
    );
    Ok(())
}

#[test]
fn rq42_classification_registry_complete() -> Result<(), Box<dyn Error>> {
    let registry = read("docs/content/docs/implemented/classification.mdx")?;
    assert!(
        registry.contains("Adding a row requires"),
        "new rows must require review (no escape hatch)"
    );
    assert!(
        registry.contains("verification rule that actually runs"),
        "new rows must name a running verification rule"
    );
    assert!(
        registry.contains("relabel"),
        "registry must cite the RQ-5.3 guard"
    );
    for anchor in ["Cargo.lock", "fixtures/**", "Vendored code", "target/"] {
        assert!(registry.contains(anchor), "registry misses {anchor}");
    }
    let mut rows = 0;
    for line in registry.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') || trimmed.contains("---") || trimmed.contains("Class |") {
            continue;
        }
        let cells: Vec<&str> = trimmed
            .split('|')
            .map(str::trim)
            .filter(|cell| !cell.is_empty())
            .collect();
        assert_eq!(
            cells.len(),
            4,
            "{trimmed} needs class/path/owner/verification"
        );
        rows += 1;
    }
    assert_eq!(rows, 8, "registry must hold exactly 8 classes");
    Ok(())
}

#[test]
fn rq52_counts_and_classification() -> Result<(), Box<dyn Error>> {
    let registry = read("docs/content/docs/implemented/classification.mdx")?;
    assert!(
        registry.contains("physical lines including comments and"),
        "counts must include comments"
    );
    assert!(registry.contains("blanks"), "counts must include blanks");
    let mut dirs = 0;
    for entry in std::fs::read_dir(repo_root().join("fixtures"))? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            assert!(
                entry.path().join("README.md").is_file(),
                "{} lacks a fixture README",
                entry.path().display()
            );
            dirs += 1;
        }
    }
    assert!(dirs >= 1, "fixtures must hold at least one suite");
    assert!(
        read(".alint.yml")?.contains("fixtures/**"),
        "fixtures must be alint-excluded test inputs"
    );
    Ok(())
}

#[test]
fn rq53_limits_discipline_no_baseline() -> Result<(), Box<dyn Error>> {
    let procedure = read("docs/content/docs/implemented/update-procedure.mdx")?;
    for clause in [
        "may raise a §5 limit",
        "arbitrary exclusion",
        "relabel handwritten code as generated",
        "reduce test assertions",
    ] {
        assert!(procedure.contains(clause), "discipline misses {clause}");
    }
    assert!(
        procedure.contains("no Alint baseline"),
        "discipline must state that no baseline exists"
    );
    assert!(
        procedure.contains("hard errors"),
        "limits must be hard errors, never grandfathered"
    );
    assert!(
        !procedure.contains("grandfathered"),
        "stale grandfathering claim must stay fixed"
    );
    assert!(
        !repo_root().join(".alint-baseline.json").is_file(),
        "baseline file must not exist"
    );
    assert!(
        read(".alint.yml")?.contains("No baseline/ratchet"),
        "alint must restate the no-baseline rule"
    );
    Ok(())
}

#[test]
fn policy_zizmor_config_matches_derived_ignores() -> Result<(), Box<dyn Error>> {
    let yaml = read(".github/workflows/ci.yml")?;
    assert!(
        yaml.contains("uses: asamarts/alint@9f9d34ba0eae3888299b9e570f43338b0e7f2cdb"),
        "full-SHA alint pin must exist"
    );
    assert!(
        !yaml.contains("asamarts/alint@v"),
        "no tag-pinned alint ref may remain"
    );
    let config = read(".zizmor.yml")?;
    assert!(
        config.contains("ignore: []"),
        "no unpinned-uses ignores may remain:\n{config}"
    );
    assert!(
        config.contains("version-policy §2"),
        "config must cite the policy"
    );
    Ok(())
}

#[test]
fn rq66_rustfmt_baseline() -> Result<(), Box<dyn Error>> {
    let format = read("rustfmt.toml")?;
    for setting in [
        "edition = \"2024\"",
        "style_edition = \"2024\"",
        "newline_style = \"Unix\"",
    ] {
        assert!(format.contains(setting), "rustfmt.toml misses {setting}");
    }
    let live = format
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .count();
    assert_eq!(live, 3, "rustfmt.toml must hold exactly 3 stable settings");
    assert!(
        read("docs/content/docs/implemented/update-procedure.mdx")?
            .contains("cargo fmt --all -- --check"),
        "qual procedure must run the fmt check"
    );
    Ok(())
}

#[test]
fn fixtures_hold_no_symlinks() -> Result<(), Box<dyn Error>> {
    let root = repo_root().join("fixtures");
    let mut pending = vec![root];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            assert!(
                !file_type.is_symlink(),
                "committed symlink {} breaks generic tree-walkers (run 36753845572); build hazards in TempDirs",
                entry.path().display()
            );
            if file_type.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    Ok(())
}
