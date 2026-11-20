// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view};
use velnor_gen::expected_with_notes;
use velnor_model::RepoPath;

const WORKFLOW: &str = ".github/workflows/rust-ci.yml";
const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::)'\nlint = true\ndoc = true\n";

fn long_named_roots(count: usize) -> Vec<String> {
    (0..count)
        .map(|index| {
            if index == 0 {
                "Cargo.toml".to_owned()
            } else {
                format!("crate-with-a-long-directory-name-{index}/Cargo.toml")
            }
        })
        .collect()
}

fn size_of(config: &str, roots: usize) -> Result<(usize, Vec<String>), Box<dyn std::error::Error>> {
    let manifests = long_named_roots(roots);
    let roots: Vec<&str> = manifests.iter().map(String::as_str).collect();
    let mut repo = view(&roots, Some(config.as_bytes()))?;
    pin_helper(&mut repo, "0.1.107")?;
    let (files, notes) = expected_with_notes(&repo)?;
    let text = files.get(&RepoPath::new(WORKFLOW)?).ok_or("no workflow")?;
    assert!(text.contains("  required:"), "no verdict job");
    Ok((text.len(), notes))
}

#[test]
fn a_parallel_lane_keeps_the_largest_fixture_under_the_workflow_size_limit() -> TestResult {
    let parallel = format!("{LANE}job = 'parallel'\n");
    let (inline, _) = size_of(LANE, 16)?;
    for (roots, base) in [(16, Some(inline)), (20, None)] {
        let (size, notes) = size_of(&parallel, roots)?;
        assert!(
            size <= velnor_render::MAX_WORKFLOW_BYTES,
            "{roots} roots: {size} bytes, notes {notes:?}"
        );
        if let Some(base) = base {
            assert!(
                size > base && size - base < 40_000,
                "{roots} roots: the lane job adds {} bytes",
                size.saturating_sub(base)
            );
        }
    }
    Ok(())
}
