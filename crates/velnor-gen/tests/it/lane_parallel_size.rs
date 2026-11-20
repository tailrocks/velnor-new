// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view};
use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::RepoPath;
use velnor_render::MAX_WORKFLOW_BYTES;

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

type Generated = Result<(usize, Vec<String>), GenerateError>;

fn generated(config: &str, roots: usize) -> Result<Generated, Box<dyn std::error::Error>> {
    let manifests = long_named_roots(roots);
    let roots: Vec<&str> = manifests.iter().map(String::as_str).collect();
    let mut repo = view(&roots, Some(config.as_bytes()))?;
    pin_helper(&mut repo, "0.1.107")?;
    match expected_with_notes(&repo) {
        Ok((files, notes)) => {
            let text = files.get(&RepoPath::new(WORKFLOW)?).ok_or("no workflow")?;
            assert!(text.contains("  required:"), "no verdict job");
            Ok(Ok((text.len(), notes)))
        }
        Err(error) => Ok(Err(error)),
    }
}

fn size_of(config: &str, roots: usize) -> Result<(usize, Vec<String>), Box<dyn std::error::Error>> {
    generated(config, roots)?.map_err(Into::into)
}

#[test]
fn a_parallel_lane_adds_a_bounded_job_to_a_sixteen_root_workflow() -> TestResult {
    let parallel = format!("{LANE}job = 'parallel'\n");
    let (inline, _) = size_of(LANE, 16)?;
    let (size, notes) = size_of(&parallel, 16)?;
    assert!(
        size <= MAX_WORKFLOW_BYTES && size > inline && size - inline < 40_000,
        "16 roots: {size} bytes against {inline} inline, notes {notes:?}"
    );
    Ok(())
}

/// The 20-root parallel workflow renders (498,858 B when this was written) and stays inside this
/// budget, 500 B under `MAX_WORKFLOW_BYTES`: any growth of a job's steps fails here instead of
/// becoming a refusal for a user's repository.
const TWENTY_ROOT_BUDGET: usize = 499_500;

#[test]
fn a_parallel_lane_renders_a_twenty_root_workflow_inside_its_budget() -> TestResult {
    let parallel = format!("{LANE}job = 'parallel'\n");
    let (size, notes) = size_of(&parallel, 20)?;
    assert!(
        size <= TWENTY_ROOT_BUDGET && TWENTY_ROOT_BUDGET < MAX_WORKFLOW_BYTES,
        "20 roots: {size} bytes against a budget of {TWENTY_ROOT_BUDGET}, notes {notes:?}"
    );
    Ok(())
}
