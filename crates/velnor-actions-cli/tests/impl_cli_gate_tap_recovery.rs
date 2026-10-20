//! Recovery must enter retained-run verification without a current-run signed artifact.

use std::error::Error;

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

#[test]
fn tap_recovery_gate_requires_positive_bounded_original_run_identity() -> Result<(), Box<dyn Error>>
{
    let tmp = fresh_tempdir("tap-recovery-gate")?;
    let root = tmp.join("velnor");
    std::fs::create_dir_all(&root)?;
    let request = root.join("package-release-policy.json");
    std::fs::write(&request, "{}")?;
    std::fs::write(root.join("tap-admission.json"), "{}")?;
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
                ("GITHUB_JOB", "preview-tap-prepare"),
                (
                    "VELNOR_TAP_ADMISSION_DIGEST",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                ),
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
        assert!(!root.join("release-publish-input").exists());
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn current_preview_gate_admits_workflow_run_and_rejects_obsolete_push() -> Result<(), Box<dyn Error>>
{
    let tmp = fresh_tempdir("tap-current-event-gate")?;
    let root = tmp.join("velnor");
    std::fs::create_dir_all(root.join("release-publish-input"))?;
    let request = root.join("package-release-policy.json");
    std::fs::write(&request, "{}")?;
    std::fs::write(root.join("tap-admission.json"), "{}")?;
    for (event, expected) in [("workflow_run", 1), ("push", 2)] {
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
                ("GITHUB_JOB", "preview-tap-prepare"),
                (
                    "VELNOR_TAP_ADMISSION_DIGEST",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                ),
                ("GITHUB_EVENT_NAME", event),
            ],
            &tmp,
        )?;
        assert_eq!(code(&output), expected, "{event}");
        if expected == 1 {
            assert!(String::from_utf8_lossy(&output.stderr).contains("internal request failed"));
        }
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn tap_prepare_rejects_ambient_credentials_before_nonexistent_source_configuration()
-> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("tap-parent-token-rejected")?;
    let root = tmp.join("velnor");
    std::fs::create_dir_all(root.join("release-publish-input"))?;
    let request = root.join("package-release-policy.json");
    std::fs::write(&request, "{}")?;
    std::fs::write(root.join("tap-admission.json"), "{}")?;
    let workspace = tmp.join("nonexistent-source");
    for (credential, expected) in [
        (None, 1),
        (Some(("GH_TOKEN", "parent-secret")), 2),
        (Some(("GITHUB_TOKEN", "")), 2),
    ] {
        let mut vars = vec![
            ("VELNOR_INTERNAL_OP", "prepare-homebrew-tap-v1"),
            ("VELNOR_REQUEST_FILE", request.to_str().ok_or("path")?),
            ("RUNNER_TEMP", tmp.to_str().ok_or("temp")?),
            ("GITHUB_RUN_ID", "42"),
            ("GITHUB_JOB", "preview-tap-prepare"),
            ("GITHUB_EVENT_NAME", "workflow_run"),
            ("GITHUB_WORKSPACE", workspace.to_str().ok_or("workspace")?),
            (
                "VELNOR_TAP_ADMISSION_DIGEST",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
        ];
        if let Some(pair) = credential {
            vars.push(pair);
        }
        let output = spawn_isolated(&[], &vars, &tmp)?;
        assert_eq!(code(&output), expected);
        assert!(!workspace.exists());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("parent-secret"));
        if credential.is_none() {
            assert!(stderr.contains("tap_admission_"), "{stderr}");
        }
    }
    cleanup(&tmp);
    Ok(())
}
