// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_gen::{Drift, drift};
use velnor_model::RepoPath;

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn check_reports_each_owned_file_that_generate_would_write_or_remove() -> TestResult {
    let workflow = RepoPath::new(".github/workflows/rust-ci.yml")?;
    let other = RepoPath::new(".github/workflows/ci.yml")?;
    let expected = BTreeMap::from([(workflow.clone(), "name: Rust CI\n".to_owned())]);
    let disk = |content: Option<&[u8]>| {
        let mut files = BTreeMap::from([(other.clone(), b"name: CI\n".to_vec())]);
        if let Some(bytes) = content {
            files.insert(workflow.clone(), bytes.to_vec());
        }
        files
    };
    assert_eq!(drift(&expected, &disk(Some(b"name: Rust CI\n"))), vec![]);
    assert_eq!(
        drift(&expected, &disk(Some(b"name: Rust CI\n# edited\n"))),
        vec![Drift::Changed(workflow.clone())]
    );
    assert_eq!(
        drift(&expected, &disk(None)),
        vec![Drift::Missing(workflow.clone())]
    );
    assert_eq!(
        drift(&BTreeMap::new(), &disk(Some(b"name: Rust CI\n"))),
        vec![Drift::Stale(workflow.clone())]
    );
    assert_eq!(drift(&BTreeMap::new(), &disk(None)), vec![]);
    Ok(())
}
