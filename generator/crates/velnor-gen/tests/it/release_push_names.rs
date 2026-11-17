// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::{GenerateError, expected_files};
use velnor_model::RepoPath;
use velnor_release::ReleaseError;

type TestResult = Result<(), Box<dyn Error>>;

const PATH: &str = ".github/workflows/extra.yml";

fn missing_name(text: &[u8], after: &str) -> Result<Option<String>, Box<dyn Error>> {
    let mut view = crate::release_fixture::release_view(
        &[crate::release_fixture::APP],
        Some(&format!("after = [{after}]\n")),
    )?;
    view.files.insert(RepoPath::new(PATH)?, text.to_vec());
    match expected_files(&view) {
        Ok(_) => Ok(None),
        Err(GenerateError::Release(ReleaseError::AfterMissingWorkflow { name })) => Ok(Some(name)),
        Err(other) => Err(other.into()),
    }
}

#[test]
fn a_push_workflow_the_old_name_scan_missed_must_be_named_in_after() -> TestResult {
    let bom = [b"\xEF\xBB\xBF".as_slice(), b"name: Extra\non: push\n"].concat();
    let cases: [(&[u8], &str); 7] = [
        (b"on: push\n", PATH),
        (b"name : Extra\non: push\n", "Extra"),
        (b"\"name\": Extra\non: push\n", "Extra"),
        (&bom, "Extra"),
        (b"name: >-\n  Extra\n  Name\non: push\n", "Extra Name"),
        (b"name: \"Extra\n  Name\"\non: push\n", "Extra Name"),
        (b"name: Extra\non: [push\n", PATH),
    ];
    for (text, expected) in cases {
        let shown = String::from_utf8_lossy(text);
        let name = missing_name(text, "\"Rust CI\"")?;
        assert_eq!(name.as_deref(), Some(expected), "{shown}");
        let after = format!("\"Rust CI\", {expected:?}");
        assert_eq!(missing_name(text, &after)?, None, "{shown}");
    }
    Ok(())
}

#[test]
fn a_workflow_without_a_name_key_is_not_a_push_workflow_unless_it_pushes() -> TestResult {
    assert_eq!(missing_name(b"on: pull_request\n", "\"Rust CI\"")?, None);
    Ok(())
}
