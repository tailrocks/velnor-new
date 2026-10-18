//! Recovery must enter retained-run verification without a current-run signed artifact.

use std::error::Error;

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

#[test]
fn tap_recovery_gate_requires_positive_bounded_original_run_identity() -> Result<(), Box<dyn Error>>
{
    let tmp = fresh_tempdir("tap-recovery-gate")?;
    let root = tmp.join("velnor");
    std::fs::create_dir_all(&root)?;
    let request = root.join("package-release-plan.json");
    std::fs::write(&request, "{}")?;
    for (event, run, attempt, expected) in [
        ("push", "1", "1", 2),
        ("pull_request", "1", "1", 2),
        ("workflow_dispatch", "", "1", 2),
        ("workflow_dispatch", "0", "1", 2),
        ("workflow_dispatch", "1", "0", 2),
        ("workflow_dispatch", "18446744073709551616", "1", 2),
        ("workflow_dispatch", "1", "4294967296", 2),
        ("workflow_dispatch", "1", "1", 1),
    ] {
        let output = spawn_isolated(
            &[],
            &[
                ("VELNOR_INTERNAL_OP", "prepare-homebrew-tap-v1"),
                (
                    "VELNOR_REQUEST_FILE",
                    request.to_str().ok_or("request path")?,
                ),
                ("RUNNER_TEMP", tmp.to_str().ok_or("temporary path")?),
                ("GITHUB_RUN_ID", "2"),
                ("GITHUB_EVENT_NAME", event),
                ("VELNOR_RELEASE_ORIGINAL_RUN_ID", run),
                ("VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT", attempt),
            ],
            &tmp,
        )?;
        assert_eq!(code(&output), expected, "{event}:{run}:{attempt}");
        if expected == 1 {
            assert!(String::from_utf8_lossy(&output.stderr).contains("internal request failed"));
        }
        assert!(!root.join("preview-publish-input").exists());
    }
    cleanup(&tmp);
    Ok(())
}
