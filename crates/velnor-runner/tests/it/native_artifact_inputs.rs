// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use super::artifact_support::{error_lines, output, run_job, write_file};
use super::exec_support::Scratch;
use super::fake_results::FakeResults;

const POLICY: &str = "::error::Unrecognized if-no-files-found input. Provided: bogus. Available options: warn,error,ignore";
const RETENTION: &str = "::error::Invalid retention-days";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_inputs_are_reported_in_the_toolkit_order_and_the_artifact_is_still_uploaded()
-> Result<()> {
    let cases = [
        (vec![("retention-days", "days")], vec![RETENTION]),
        (vec![("if-no-files-found", "bogus")], vec![POLICY]),
        (
            vec![("retention-days", "days"), ("if-no-files-found", "bogus")],
            vec![POLICY, RETENTION],
        ),
    ];
    for (extra, expected) in cases {
        let fake = FakeResults::start().await?;
        let scratch = Scratch::new()?;
        write_file(&scratch, "a.txt", b"a")?;
        let mut inputs = vec![("name", "bundle"), ("path", "a.txt")];
        inputs.extend_from_slice(&extra);
        let ran = run_job(&scratch, "artifact_upload", &fake.url(), &inputs)?;
        assert_eq!(ran.code, Some(1), "{extra:?}: {}", ran.stdout);
        assert_eq!(error_lines(&ran.stdout), expected, "{extra:?}");
        assert_eq!(fake.artifacts()?.len(), 1, "{extra:?}: {}", ran.stdout);
        assert!(fake.calls("CreateArtifact")?[0]["expires_at"].is_null());
        assert!(output(&ran, "artifact-id").is_some(), "{extra:?}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retention_of_zero_sets_no_expiry_and_does_not_fail() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "a.txt", b"a")?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[
            ("name", "bundle"),
            ("path", "a.txt"),
            ("retention-days", "0"),
        ],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert!(fake.calls("CreateArtifact")?[0]["expires_at"].is_null());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_input_given_empty_is_not_the_same_as_an_input_left_out() -> Result<()> {
    let cases = [
        (
            "if-no-files-found",
            "::error::Unrecognized if-no-files-found input. Provided: . Available options: warn,error,ignore",
            true,
        ),
        (
            "overwrite",
            "::error::Input does not meet YAML 1.2 \"Core Schema\" specification: overwrite%0ASupport boolean input list: `true | True | TRUE | false | False | FALSE`",
            false,
        ),
        (
            "name",
            "::error::Provided artifact name input during validation is empty",
            false,
        ),
    ];
    for (input, line, uploads) in cases {
        let fake = FakeResults::start().await?;
        let scratch = Scratch::new()?;
        write_file(&scratch, "a.txt", b"a")?;
        let mut inputs = vec![("name", "bundle"), ("path", "a.txt")];
        inputs.retain(|(name, _)| *name != input);
        inputs.push((input, ""));
        let ran = run_job(&scratch, "artifact_upload", &fake.url(), &inputs)?;
        assert_eq!(ran.code, Some(1), "{input}: {}", ran.stdout);
        assert_eq!(error_lines(&ran.stdout), [line], "{input}");
        assert_eq!(
            fake.artifacts()?.len(),
            usize::from(uploads),
            "{input}: {}",
            ran.stdout
        );
    }
    Ok(())
}
