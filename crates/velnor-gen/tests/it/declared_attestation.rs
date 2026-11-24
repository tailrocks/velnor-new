// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_with_notes;
use velnor_model::RepoPath;

use crate::release_fixture::release_view;

type TestResult = Result<(), Box<dyn Error>>;

const BUILD_AND_ATTEST: &str = r#"
[[workflow]]
file = "build-assets"
name = "Build assets"

[[workflow.job]]
id = "build"
permissions = { contents = "read", id-token = "write", attestations = "write" }

[[workflow.job.step]]
uses = "checkout"

[[workflow.job.step]]
name = "Build"
run = "cargo build --release --locked"

[[workflow.job.step]]
id = "upload"
uses = "upload-artifact"
with = { name = "assets", path = "target/release/app" }

[[workflow.job.step]]
uses = "attest"
with = { subject-path = "target/release/app" }
"#;

const SPLIT_BUILD_AND_ATTEST: &str = r#"
[[workflow]]
file = "build-assets"
name = "Build assets"

[[workflow.job]]
id = "build"
permissions = { contents = "read" }

[[workflow.job.step]]
uses = "checkout"

[[workflow.job.step]]
name = "Build"
run = "cargo build --release --locked"

[[workflow.job.step]]
id = "upload"
uses = "upload-artifact"
with = { name = "assets", path = "target/release/app" }

[[workflow.job]]
id = "attest"
needs = ["build"]
permissions = { contents = "read", id-token = "write", attestations = "write" }

[[workflow.job.step]]
uses = "download-artifact"
with = { name = "assets", path = "target/release" }

[[workflow.job.step]]
uses = "attest"
with = { subject-path = "target/release/app" }
"#;

fn notes(config: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let mut view = release_view(&[], None)?;
    view.files.insert(
        RepoPath::new(".velnor/workflows.toml")?,
        config.as_bytes().to_vec(),
    );
    Ok(expected_with_notes(&view)?.1)
}

#[test]
fn a_job_that_builds_and_attests_with_id_token_write_warns_with_the_split() -> TestResult {
    let notes = notes(BUILD_AND_ATTEST)?;
    let warning = notes
        .iter()
        .find(|note| note.contains("build-assets"))
        .ok_or("no warning names the workflow")?;
    assert!(
        warning.contains("job build") && warning.contains("split"),
        "{warning}"
    );
    Ok(())
}

#[test]
fn a_split_build_and_attest_has_no_attestation_warning() -> TestResult {
    let notes = notes(SPLIT_BUILD_AND_ATTEST)?;
    assert!(
        notes.iter().all(|note| !note.contains("warning")),
        "{notes:?}"
    );
    Ok(())
}
