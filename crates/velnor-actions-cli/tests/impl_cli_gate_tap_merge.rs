//! The merge gate must admit normal GitHub Actions output-file environments.

use std::error::Error;

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

#[test]
fn merge_gate_accepts_github_output_environment() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("tap-merge-github-output")?;
    let root = tmp.join("velnor");
    let candidate = root.join("tap-candidate");
    std::fs::create_dir_all(root.join("release-publish-input"))?;
    std::fs::create_dir_all(&candidate)?;
    for relative in [
        "package-release-policy.json",
        "tap-pr.json",
        "tap-candidate/formula.rb",
        "tap-candidate/candidate.json",
    ] {
        std::fs::write(root.join(relative), "{}")?;
    }
    let output_file = tmp.join("github-output");
    std::fs::write(&output_file, "")?;
    let request = root.join("package-release-policy.json");
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "merge-homebrew-tap-pr-v1"),
            (
                "VELNOR_REQUEST_FILE",
                request.to_str().ok_or("request path")?,
            ),
            ("RUNNER_TEMP", tmp.to_str().ok_or("temporary path")?),
            ("GITHUB_RUN_ID", "42"),
            ("GITHUB_EVENT_NAME", "workflow_run"),
            ("GITHUB_OUTPUT", output_file.to_str().ok_or("output path")?),
            ("VELNOR_TAP_MERGE_TOKEN", "merge-token"),
            ("VELNOR_TAP_PROTECTION_TOKEN", "observer-token"),
            ("VELNOR_TAP_PROTECTION_APP_ID", "789"),
            ("VELNOR_TAP_PROTECTION_APP_SLUG", "tap-observer"),
            ("VELNOR_TAP_PROTECTION_INSTALLATION_ID", "987"),
            ("VELNOR_TAP_APP_ID", "123"),
            ("VELNOR_TAP_APP_SLUG", "tap-bot"),
            ("VELNOR_TAP_INSTALLATION_ID", "456"),
            ("VELNOR_TAP_COMPLETION_APP_ID", "789"),
            ("VELNOR_TAP_REVIEWER_APP_ID_0", "790"),
            ("VELNOR_TAP_POLICY", "{}"),
            ("GH_TOKEN", "source-token"),
        ],
        &tmp,
    )?;
    assert_eq!(code(&output), 1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("internal request failed"), "{stderr}");
    assert!(
        !stderr.contains("Usage:"),
        "merge gate rejected request: {stderr}"
    );
    cleanup(&tmp);
    Ok(())
}
