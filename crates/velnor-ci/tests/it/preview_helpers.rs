// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::{Command, Output};

use crate::support::fresh_dir;

const ID: &str = "VELNOR_EVIDENCE_ARTIFACT_ID";

fn validate(id: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command.arg("validate-artifact-id").env_remove(ID);
    if let Some(id) = id {
        command.env(ID, id);
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn canonical_ids_up_to_the_javascript_safe_integer_pass() {
    for id in ["1", "42", "9007199254740991", "999999999999999"] {
        let output = validate(Some(id));
        assert!(output.status.success(), "{id}: {output:?}");
    }
}

#[test]
fn every_other_id_fails_closed() {
    for id in [
        "",
        "0",
        "01",
        "-1",
        "+1",
        "1.5",
        "12a",
        " 1",
        "1 ",
        "1\n",
        "9007199254740992",
        "10000000000000000",
        "99999999999999999",
        "\u{661}",
    ] {
        let output = validate(Some(id));
        assert_eq!(output.status.code(), Some(1), "{id:?}: {output:?}");
    }
    assert_eq!(validate(None).status.code(), Some(1));
}

fn fake_mise(dir: &Path) -> std::path::PathBuf {
    let bin = dir.join("bin");
    fs::create_dir(&bin).unwrap_or_else(|e| unreachable!("{e}"));
    let mise = bin.join("mise");
    fs::write(
        &mise,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$MISE_LOG\"\nexit \"${MISE_EXIT:-0}\"\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&mise, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    bin
}

fn install_gh(label: &str, token: Option<&str>, exit: &str) -> (Output, String) {
    let dir = fresh_dir("velnor-ci-install-gh", label);
    let bin = fake_mise(&dir);
    let log = dir.join("mise.log");
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args(["install-gh", "--version", "2.103.3"])
        .env("PATH", &bin)
        .env("MISE_LOG", &log)
        .env("MISE_EXIT", exit)
        .env_remove("GITHUB_TOKEN");
    if let Some(token) = token {
        command.env("GITHUB_TOKEN", token);
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    (output, fs::read_to_string(&log).unwrap_or_default())
}

#[test]
fn install_gh_runs_mise_with_the_pinned_version_and_no_ambient_config() {
    let (output, log) = install_gh("ok", Some("t"), "0");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        log,
        "--no-config\n--no-env\n--no-hooks\ninstall\ngh@2.103.3\n"
    );
}

#[test]
fn install_gh_refuses_a_missing_or_empty_token_without_running_mise() {
    for token in [None, Some("")] {
        let (output, log) = install_gh("no-token", token, "0");
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(log.is_empty(), "{log}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("GITHUB_TOKEN"));
    }
}

#[test]
fn install_gh_fails_when_mise_fails() {
    let (output, _) = install_gh("mise-fails", Some("t"), "3");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}

#[test]
fn install_gh_refuses_a_version_that_is_not_a_plain_version() {
    for version in [
        "",
        "latest",
        "2.103",
        "2.103.3 --foo",
        "../2.103.3",
        "2.103.3-rc1",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["install-gh", "--version", version])
            .env("GITHUB_TOKEN", "t")
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(output.status.code(), Some(2), "{version:?}: {output:?}");
    }
}

const BUNDLE_NAME: &str = "SDK_PREVIEW_RECEIPT.json.sigstore.jsonl";

fn stage(temp: &Path, bundle: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .arg("stage-bundle")
        .env("RUNNER_TEMP", temp)
        .env_remove("BUNDLE_PATH");
    if let Some(bundle) = bundle {
        command.env("BUNDLE_PATH", bundle);
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_regular_nonempty_bundle_is_staged_read_only_and_exact() {
    let dir = fresh_dir("velnor-ci-stage", "ok");
    let temp = dir.join("temp");
    fs::create_dir(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    let bundle = dir.join("bundle.json");
    fs::write(&bundle, b"{\"bundle\":1}\n").unwrap_or_else(|e| unreachable!("{e}"));
    let output = stage(&temp, Some(&bundle));
    assert!(output.status.success(), "{output:?}");
    let staged = temp.join(BUNDLE_NAME);
    assert_eq!(
        fs::read(&staged).unwrap_or_default(),
        b"{\"bundle\":1}\n".to_vec()
    );
    let mode = fs::metadata(&staged)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o400);
}

#[test]
fn staging_fails_closed_on_an_unusable_bundle_or_an_occupied_destination() {
    let dir = fresh_dir("velnor-ci-stage", "bad");
    let temp = dir.join("temp");
    fs::create_dir(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    let real = dir.join("real.json");
    fs::write(&real, b"x").unwrap_or_else(|e| unreachable!("{e}"));
    let empty = dir.join("empty.json");
    fs::write(&empty, b"").unwrap_or_else(|e| unreachable!("{e}"));
    let link = dir.join("link.json");
    symlink(&real, &link).unwrap_or_else(|e| unreachable!("{e}"));
    let nothing = dir.join("absent.json");
    for bundle in [Some(&empty), Some(&link), Some(&nothing), Some(&dir), None] {
        let output = stage(&temp, bundle.map(std::path::PathBuf::as_path));
        assert_eq!(output.status.code(), Some(1), "{bundle:?}: {output:?}");
        assert!(!temp.join(BUNDLE_NAME).exists(), "{bundle:?}");
    }
    let dangling = temp.join(BUNDLE_NAME);
    symlink(dir.join("elsewhere"), &dangling).unwrap_or_else(|e| unreachable!("{e}"));
    let output = stage(&temp, Some(&real));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(!dir.join("elsewhere").exists());
    fs::remove_file(&dangling).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(&dangling, b"old").unwrap_or_else(|e| unreachable!("{e}"));
    let output = stage(&temp, Some(&real));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(fs::read(&dangling).unwrap_or_default(), b"old".to_vec());
}
