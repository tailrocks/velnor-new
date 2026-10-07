//! P12 mutants-scope case: risk-wiring coverage of the manual-only config.

use std::error::Error;
use std::path::{Path, PathBuf};

/// Files matched by one mutants scope glob (exact or `dir/**/*.rs`).
fn glob_hits(root: &Path, pattern: &str) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    if let Some((prefix, suffix)) = pattern.split_once("**") {
        assert_eq!(suffix, "/*.rs", "unsupported test glob {pattern}");
        let mut hits = Vec::new();
        let mut pending = vec![root.join(prefix)];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    pending.push(entry.path());
                } else if entry.path().extension().is_some_and(|ext| ext == "rs") {
                    hits.push(entry.path());
                }
            }
        }
        return Ok(hits);
    }
    let path = root.join(pattern);
    Ok(if path.is_file() {
        vec![path]
    } else {
        Vec::new()
    })
}

#[test]
fn mutant_scope_covers_risk_wiring() -> Result<(), Box<dyn Error>> {
    let root = crate::impl_repo_policy::repo_root();
    let mutants = crate::impl_repo_policy::read(".cargo/mutants.toml")?;
    assert!(mutants.contains("NOT wired into CI"), "manual-only status");
    let mut in_scope = false;
    let mut globs = 0;
    for line in mutants.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("examine_globs") {
            in_scope = true;
        } else if in_scope {
            if trimmed.starts_with(']') {
                break;
            }
            for part in trimmed.split('"').skip(1).step_by(2) {
                globs += 1;
                assert!(!glob_hits(&root, part)?.is_empty(), "dangling glob {part}");
            }
        }
    }
    assert!(globs >= 20, "only {globs} scope globs");
    for file in [
        "crates/core/velnor-actions-contract/src/strict_json.rs",
        "crates/core/velnor-actions-contract/src/ids/**/*.rs",
        "crates/adapters/velnor-actions-rust/src/identity.rs",
        "crates/adapters/velnor-actions-mise-catalog/src/toolfiles.rs",
        "crates/services/velnor-actions-orchestrator-selection/src/select.rs",
        "crates/services/velnor-actions-orchestrator-selection/src/select_affected.rs",
        "crates/services/velnor-actions-orchestrator/src/cover_identity.rs",
        "crates/services/velnor-actions-orchestrator/src/merge_request.rs",
        "crates/services/velnor-actions-orchestrator-pins/src/pins.rs",
        "crates/services/velnor-actions-orchestrator/src/validate.rs",
        "crates/services/velnor-actions-orchestrator/src/wire_w1.rs",
    ] {
        assert!(mutants.contains(file), "scope misses {file}");
    }
    Ok(())
}
