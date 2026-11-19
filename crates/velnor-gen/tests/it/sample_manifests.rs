// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::{RepoPath, RepoView};
use velnor_rust::metadata_roots;

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn a_recorded_sample_manifest_in_a_real_repository_is_not_a_workspace_root() -> TestResult {
    let mut view = RepoView::default();
    // A recorded sample view holds a manifest whose crate has no sources.
    for path in [
        "Cargo.toml",
        "src/lib.rs",
        "generator/samples/x/view/Cargo.toml",
    ] {
        view.tracked_paths
            .insert(RepoPath::new(path).map_err(|e| e.to_string())?);
    }
    let roots = metadata_roots(&view)?;
    assert_eq!(
        roots,
        vec![RepoPath::new("Cargo.toml").map_err(|e| e.to_string())?],
        "a sample manifest was taken for a workspace root"
    );
    Ok(())
}
