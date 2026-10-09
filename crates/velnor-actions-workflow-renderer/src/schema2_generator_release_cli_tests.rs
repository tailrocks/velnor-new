//! Isolated Mise and GitHub CLI dispatch tests for generated release scripts.

use super::{Failure, PINNED_MISE_ARGUMENTS, Scratch, install_mock_gh, manifest};
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

pub(super) fn isolate_gh_environment(
    command: &mut Command,
    root: &Path,
) -> Result<(), Box<dyn Error>> {
    let home = root.join("isolated-home");
    let config = root.join("isolated-gh-config");
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&config)?;
    command
        .env("HOME", home)
        .env("GH_CONFIG_DIR", config)
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_ENTERPRISE_TOKEN");
    Ok(())
}

pub(super) fn install_mock_mise(root: &Path) -> Result<(), Box<dyn Error>> {
    let path = root.join("mock-bin/mise");
    fs::write(
        &path,
        "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"$MISE_CALLS\"\n[ \"$1 $2 $3 $4 $5 $6 $7\" = '--no-config --no-env --no-hooks exec gh@2.102.0 -- gh' ] || exit 70\nshift 7\ntest -x \"$MOCK_GH\"\nexec \"$MOCK_GH\" \"$@\"\n",
    )?;
    let mut permissions = fs::metadata(&path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

pub(super) fn assert_pinned_publish_calls(
    root: &Path,
    case: Failure,
) -> Result<(), Box<dyn Error>> {
    let log = root.join("mise-calls");
    let calls = if log.exists() {
        fs::read_to_string(log)?
    } else {
        String::new()
    };
    let prefix = format!("{PINNED_MISE_ARGUMENTS} gh ");
    for call in calls.lines() {
        assert!(
            call.starts_with(&prefix),
            "unpinned Mise invocation: {call}"
        );
    }
    let external_calls = calls.lines().count();
    let has_local_input_failure = matches!(
        case,
        Failure::WrongSidecarName | Failure::MissingBinary | Failure::StaleManifest
    );
    let has_preflight_failure = matches!(
        case,
        Failure::UnauthorizedTag | Failure::ForbiddenTag | Failure::TransientTag
    );
    let has_required_failure = case == Failure::DuplicateRequired;
    if has_local_input_failure {
        assert_eq!(external_calls, 0, "{calls}");
    } else if has_preflight_failure {
        assert_eq!(external_calls, 1, "{calls}");
    } else if has_required_failure {
        // Merged order: tag + release preflights, main-tip check, CI runs, then the
        // Required-jobs read that detects the duplicate and aborts before mutations.
        assert_eq!(external_calls, 5, "{calls}");
        assert!(
            calls
                .lines()
                .last()
                .is_some_and(|call| call.contains("attempts/1/jobs"))
        );
        assert!(!calls.contains("release create"), "{calls}");
    } else {
        assert!(
            calls
                .lines()
                .any(|call| call.starts_with(&format!("{prefix}release create "))),
            "pinned draft creation missing: {calls}"
        );
        assert!(
            calls
                .lines()
                .any(|call| call.starts_with(&format!("{prefix}release view "))),
            "pinned release ID resolution missing: {calls}"
        );
        assert!(
            calls
                .lines()
                .any(|call| call.starts_with(&format!("{prefix}release upload "))),
            "pinned asset upload missing: {calls}"
        );
        let release_reads = calls
            .lines()
            .filter(|call| call.contains("api repos/tailrocks/velnor-new/releases/123"))
            .count();
        let expected_reads = match case {
            Failure::WrongDraftReleaseId
            | Failure::WrongDraftRepository
            | Failure::WrongDraftApiPath
            | Failure::WrongDraftTag
            | Failure::WrongDraftSource
            | Failure::WrongDraftDigest
            | Failure::WrongDraftSize
            | Failure::WrongDraftInventory
            | Failure::UnknownDraftAsset
            | Failure::WrongDraftAssetName
            | Failure::WrongDraftAssetState
            | Failure::TagMovedBeforePublish => 1,
            _ => 3,
        };
        assert_eq!(release_reads, expected_reads, "{calls}");
    }
    Ok(())
}

#[test]
fn attestation_verification_uses_only_the_pinned_mock_cli() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    super::write_attestation_files(&scratch.0)?;
    install_mock_gh(&scratch.0)?;
    let gh_function = super::super::workflow_steps::gh_function(&super::test_pins().gh_argv)?;
    let script = format!("{gh_function}\n{}", manifest::attestation_bundle_script());
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(script)
        .current_dir(&scratch.0)
        .env("PATH", super::path_with_mock_gh(&scratch.0)?)
        .env("GITHUB_REPOSITORY", super::REPOSITORY)
        .env("GITHUB_SHA", super::SOURCE_SHA)
        .env("GITHUB_WORKFLOW_SHA", super::SOURCE_SHA)
        .env("GH_CALLS", scratch.0.join("gh-calls"))
        .env("MISE_CALLS", scratch.0.join("mise-calls"))
        .env("MOCK_GH", scratch.0.join("mock-bin/gh"));
    isolate_gh_environment(&mut command, &scratch.0)?;
    let output = command.output()?;
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_pinned_attestation_calls(&scratch.0)
}

fn assert_pinned_attestation_calls(root: &Path) -> Result<(), Box<dyn Error>> {
    let mise = fs::read_to_string(root.join("mise-calls"))?;
    let gh = fs::read_to_string(root.join("gh-calls"))?;
    assert_eq!(mise.lines().count(), 10, "{mise}");
    assert_eq!(gh.lines().count(), 10, "{gh}");
    let prefix = format!("{PINNED_MISE_ARGUMENTS} gh attestation verify ");
    assert!(mise.lines().all(|call| call.starts_with(&prefix)), "{mise}");
    assert!(
        gh.lines()
            .all(|call| call.starts_with("attestation verify ")),
        "{gh}"
    );
    Ok(())
}

#[test]
fn fake_mise_rejects_an_unpinned_gh_without_dispatch() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    install_mock_gh(&scratch.0)?;
    let mut command = Command::new(scratch.0.join("mock-bin/mise"));
    command
        .args([
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "gh@2.101.0",
            "--",
            "gh",
            "api",
            "test",
        ])
        .env("MISE_CALLS", scratch.0.join("mise-calls"))
        .env("GH_CALLS", scratch.0.join("gh-calls"))
        .env("MOCK_GH", scratch.0.join("mock-bin/gh"));
    isolate_gh_environment(&mut command, &scratch.0)?;
    assert!(!command.output()?.status.success());
    assert!(fs::read_to_string(scratch.0.join("mise-calls"))?.contains("gh@2.101.0"));
    assert!(!scratch.0.join("gh-calls").exists());
    Ok(())
}
