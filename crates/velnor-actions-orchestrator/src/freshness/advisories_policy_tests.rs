use std::fs;
use std::path::Path;

use super::*;

fn check_policy(contents: &str, relative: &Path) -> FreshnessContext {
    let root = tempfile::tempdir().expect("policy fixture");
    let path = root.path().join(relative);
    fs::create_dir_all(path.parent().expect("policy parent")).expect("policy directory");
    fs::write(path, contents).expect("policy file");
    let mut ctx = FreshnessContext::new(root.path(), false, false);
    check_deny_policy_at(&mut ctx, relative);
    ctx
}

#[test]
fn ignore_requires_an_explicit_empty_array_for_root_and_nested_files() {
    for (path, contents) in [
        (Path::new("deny.toml"), "[advisories]\n"),
        (Path::new("deny.toml"), "[advisories]\nignore = false\n"),
        (Path::new("deny.toml"), "[advisories]\nignore = \"\"\n"),
        (Path::new("deny.toml"), "[advisories]\nignore = {}\n"),
        (
            Path::new("crates/runner/deny.toml"),
            "[advisories]\nignore = false\n",
        ),
    ] {
        let ctx = check_policy(contents, path);
        assert!(
            ctx.failures
                .iter()
                .any(|row| row.contains("[advisories].ignore must be an empty array")),
            "invalid ignore value passed for {path:?}: {:?}",
            ctx.failures
        );
    }
}

#[test]
fn empty_ignore_array_passes_and_nonempty_subject_keeps_policy_path() {
    let clean = check_policy("[advisories]\nignore = []\n", Path::new("deny.toml"));
    assert!(clean.failures.is_empty(), "{:?}", clean.failures);
    let ignored = check_policy(
        "[advisories]\nignore = [\"RUSTSEC-2026-0001\"]\n",
        Path::new("deny.toml"),
    );
    assert!(
        ignored
            .failures
            .iter()
            .any(|row| row.contains("deny.toml:RUSTSEC-2026-0001")),
        "root policy subject lost its path: {:?}",
        ignored.failures
    );
}

#[test]
fn live_scan_uses_locked_resolution_and_workspace_scoped_inputs() {
    let root = tempfile::tempdir().expect("live scan fixture");
    let ctx = FreshnessContext::new(root.path(), false, true);
    for relative in [Path::new(""), Path::new("crates/runner")] {
        let command = live_command(&ctx, relative);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(args[0], "--locked");
        assert_eq!(args[1], "--config");
        assert_eq!(args[2], "deny.toml");
        assert_eq!(args[3], "--manifest-path");
        assert_eq!(args[4], "Cargo.toml");
        assert_eq!(&args[5..], ["check", "advisories"]);
        assert_eq!(
            command.get_current_dir(),
            Some(root.path().join(relative).as_path())
        );
    }
}

#[cfg(unix)]
#[test]
fn live_scan_resolves_inputs_when_root_is_relative() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let working_directory = std::env::current_dir()?;
    let root = tempfile::Builder::new()
        .prefix(".freshness-relative-root-")
        .tempdir_in(&working_directory)?;
    let relative_root = Path::new(root.path().file_name().ok_or("missing fixture name")?);
    let workspace_relative = Path::new("crates/runner");
    let workspace = root.path().join(workspace_relative);
    fs::create_dir_all(&workspace)?;
    fs::write(workspace.join("Cargo.toml"), "[workspace]\nmembers = []\n")?;
    fs::write(workspace.join("deny.toml"), "[advisories]\nignore = []\n")?;

    let executable_directory = tempfile::tempdir()?;
    let executable = executable_directory.path().join("cargo-deny");
    fs::write(
        &executable,
        "#!/bin/sh\ntest -f deny.toml && test -f Cargo.toml || exit 23\n\
         pwd > \"$FRESHNESS_CAPTURE\"\nprintf '%s\\n' \"$@\" >> \"$FRESHNESS_CAPTURE\"\n",
    )?;
    let mut permissions = fs::metadata(&executable)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions)?;
    let capture = executable_directory.path().join("captured-argv");
    let ctx = FreshnessContext::new(relative_root, false, true);
    let output = live_command(&ctx, workspace_relative)
        .env("PATH", executable_directory.path())
        .env("FRESHNESS_CAPTURE", &capture)
        .output()?;

    assert!(
        output.status.success(),
        "fake cargo-deny failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines = fs::read_to_string(capture)?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(lines[0], workspace.display().to_string());
    assert_eq!(
        &lines[1..],
        [
            "--locked",
            "--config",
            "deny.toml",
            "--manifest-path",
            "Cargo.toml",
            "check",
            "advisories"
        ]
    );
    Ok(())
}
