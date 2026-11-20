// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::base_save::pinned;
use crate::facts::TestResult;
use velnor_gen::expected_files;

fn cleanup(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    expected_files(&pinned("0.1.102", config)?)?
        .remove(&velnor_render::DELTA_CLEANUP_PATH)
        .ok_or_else(|| "no cleanup workflow".into())
}

#[test]
fn the_cleanup_job_may_read_the_pull_request_it_trims_and_nothing_else_extra() -> TestResult {
    // trim-deltas reads GET /pulls/{n} (and /commits/{sha}/pulls), which a private
    // repository refuses with 403 unless the token holds pull-requests: read.
    let text = cleanup("")?;
    assert!(
        text.contains(
            "    permissions:\n      actions: write\n      contents: read\n      pull-requests: read\n    cache-mode: read\n"
        ),
        "{text}"
    );
    assert!(text.contains("\npermissions: {}\n"), "{text}");
    assert_eq!(text.matches("pull-requests:").count(), 1, "{text}");
    Ok(())
}

#[test]
fn the_cleanup_job_runs_on_the_configured_runner_and_hosted_only_when_nothing_is_set() -> TestResult
{
    let configured = cleanup("runner = \"ubuntu-26.04-scale-set\"\n")?;
    assert!(
        configured.contains("    runs-on: ubuntu-26.04-scale-set\n"),
        "{configured}"
    );
    assert!(!configured.contains("ubuntu-latest"), "{configured}");
    let default = cleanup("")?;
    assert!(
        default.contains("    runs-on: ubuntu-latest\n"),
        "{default}"
    );
    Ok(())
}
