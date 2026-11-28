// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use super::artifact_support::{JOB_RUN, OLD_JOB, read_file, run_job};
use super::exec_support::Scratch;
use super::fake_results::FakeResults;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_blob_file_name_ends_where_the_pinned_toolkit_regex_ends_it() -> Result<()> {
    let cases = [
        ("attachment; filename=\"it's.txt\"", "it"),
        ("attachment; filename=\"report;final.txt\"", "report"),
        ("attachment; filename=\"say\"hi.txt\"", "say"),
        ("attachment; FileName = 'plain.txt'", "plain.txt"),
        ("attachment; filename*filename=x.txt", "artifact"),
        ("attachment; xfilename=a.txt", "a.txt"),
        ("attachment; xfilename*=UTF-8''b.txt", "b.txt"),
        ("attachment; filename*=UTF-8''a;b.txt", "a"),
        ("attachment; filename=café.txt", "cafÃ©.txt"),
        ("attachment; filename=a.txt\u{85}", "a.txt\u{c2}\u{85}"),
    ];
    for (disposition, expected) in cases {
        let fake = FakeResults::start().await?;
        fake.seed_raw(
            "ci-metrics-raw",
            JOB_RUN,
            OLD_JOB,
            b"raw bytes".to_vec(),
            disposition,
        )?;
        let scratch = Scratch::new()?;
        let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
        assert_eq!(ran.code, Some(0), "{disposition}: {}", ran.stdout);
        assert_eq!(
            read_file(scratch.workspace().join("restored").join(expected))?,
            b"raw bytes",
            "{disposition}"
        );
    }
    Ok(())
}
