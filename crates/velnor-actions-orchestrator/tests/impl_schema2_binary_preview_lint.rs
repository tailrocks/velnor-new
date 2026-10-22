//! The `github-env` audit zizmor used to run is enforced by jactionlint.
use crate::impl_common::TestResult;

/// Run pinned jactionlint at `profile: default` (zizmor's audits) on one file.
fn real_jactionlint_default(
    dir: &std::path::Path,
    file: &str,
) -> Result<(bool, String), Box<dyn std::error::Error>> {
    let output = velnor_actions_mise::PinnedToolExec::new(
        vec![velnor_actions_mise::PinnedTool::Jactionlint],
        std::ffi::OsStr::new("jactionlint"),
        [
            "--profile",
            "default",
            "--no-online",
            "--shellcheck=",
            "--pyflakes=",
            "--no-color",
            "--format",
            "oneline",
            file,
        ]
        .into_iter()
        .map(Into::into)
        .collect(),
    )?
    .command(&velnor_actions_mise::ToolCatalog::pinned())?
    .with_cwd(dir.to_path_buf())
    .run_bounded(128 * 1024, std::time::Duration::from_secs(30))?;
    Ok((
        output.success,
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    ))
}

#[test]
fn unrelated_privileged_environment_write_still_fails_real_jactionlint() -> TestResult {
    let dir = tempfile::tempdir()?;
    let unsafe_write = "name: Unsafe environment write\n\"on\": pull_request_target\npermissions: {}\njobs:\n  unsafe:\n    runs-on: ubuntu-26.04\n    steps:\n      - run: echo \"$UNTRUSTED\" >> \"$GITHUB_ENV\"\n";
    std::fs::write(dir.path().join("unsafe.yml"), unsafe_write)?;
    let (success, diagnostics) = real_jactionlint_default(dir.path(), "unsafe.yml")?;
    assert!(!success, "{diagnostics}");
    assert!(diagnostics.contains("[github-env]"), "{diagnostics}");
    // The narrow zizmor annotations Velnor emits keep working: jactionlint
    // honours `# zizmor: ignore[...]` for the audit it replaces.
    std::fs::write(
        dir.path().join("annotated.yml"),
        unsafe_write.replace(
            ">> \"$GITHUB_ENV\"\n",
            ">> \"$GITHUB_ENV\" # zizmor: ignore[github-env]\n",
        ),
    )?;
    let (_, annotated) = real_jactionlint_default(dir.path(), "annotated.yml")?;
    assert!(!annotated.contains("[github-env]"), "{annotated}");
    Ok(())
}
