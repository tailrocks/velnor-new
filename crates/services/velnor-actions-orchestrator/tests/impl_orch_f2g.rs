//! F2 structural scan (overflow): CLI carries no stack vocabulary.
//!
//! Split from `impl_orch_f2a` (size gate): tree-scan helper plus its one test.

use std::path::PathBuf;

use crate::impl_common::TestResult;

/// All `.rs` files under a crate-relative directory, recursively.
fn tree_rs(relative: &str) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    let mut out = Vec::new();
    let mut pending = vec![root];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

#[test]
fn cli_carries_no_stack_flags() -> TestResult {
    for path in tree_rs("../../apps/velnor-actions-cli/src")? {
        let text = std::fs::read_to_string(&path)?;
        assert!(
            !text.to_lowercase().contains("stack"),
            "stack flag in {}",
            path.display()
        );
    }
    Ok(())
}
