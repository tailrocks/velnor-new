//! Executed fixture tests for controller identity and result classification.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{render, scripts};
use crate::yaml::Yaml;
use velnor_actions_contract::StepKind;

#[path = "schema2_mbx_cancel_probe_test_support.rs"]
mod test_support;
use test_support::{FAKE_GH, FAKE_REALPATH, FAKE_STAT};

#[path = "schema2_mbx_cancel_probe_cache_snapshot_tests.rs"]
mod cache_snapshots;
#[path = "schema2_mbx_cancel_probe_classification_tests.rs"]
mod classifications;
#[path = "schema2_mbx_cancel_probe_controller_fixture_tests.rs"]
mod controller_fixtures;
#[path = "schema2_mbx_cancel_probe_controller_transport_cases.rs"]
mod controller_transport;
#[path = "schema2_mbx_cancel_probe_key_fixture_tests.rs"]
mod key_fixtures;
#[path = "schema2_mbx_cancel_probe_native_render_tests.rs"]
mod native_render_tests;
#[path = "schema2_mbx_cancel_probe_observer_fixture_tests.rs"]
mod observer_fixtures;
#[path = "schema2_mbx_cancel_probe_pre_save_fixture_tests.rs"]
mod pre_save_fixtures;
#[path = "schema2_mbx_cancel_probe_private_io_tests.rs"]
mod private_io_fixtures;
#[path = "schema2_mbx_cancel_probe_receipt_fixture_tests.rs"]
mod receipt_fixtures;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub(super) fn temp_dir(label: &str) -> io::Result<PathBuf> {
    let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().canonicalize()?.join(format!(
        "velnor-mbx-cancel-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&path)?;
    Ok(path)
}

pub(super) fn fake_bin(root: &Path) -> io::Result<PathBuf> {
    let bin = root.join("bin");
    fs::create_dir(&bin)?;
    let gh = bin.join("gh");
    fs::write(&gh, FAKE_GH)?;
    let sha256sum = bin.join("sha256sum");
    fs::write(
        &sha256sum,
        "#!/usr/bin/env bash\nexec shasum -a 256 \"$@\"\n",
    )?;
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755))?;
    fs::set_permissions(&sha256sum, fs::Permissions::from_mode(0o755))?;
    let realpath = bin.join("realpath");
    fs::write(&realpath, FAKE_REALPATH)?;
    let stat = bin.join("stat");
    fs::write(&stat, FAKE_STAT)?;
    fs::set_permissions(&realpath, fs::Permissions::from_mode(0o755))?;
    fs::set_permissions(&stat, fs::Permissions::from_mode(0o755))?;
    Ok(bin)
}

pub(super) fn run_bash(
    script: &str,
    cwd: &Path,
    bin: &Path,
    envs: &[(String, String)],
) -> io::Result<Output> {
    let mut path = bin.as_os_str().to_os_string();
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let mut command = Command::new("bash");
    let script = format!(
        "{}\n{}\n{script}",
        crate::schema2::mbx_stock_restore::STOCK_RESTORE_CLASSIFIER_SCRIPT,
        super::private_io::PRIVATE_IO_HELPERS
    );
    command
        .env_clear()
        .env("PATH", path)
        .env("HOME", cwd)
        .env("TMPDIR", cwd)
        .current_dir(cwd)
        .args(["-c", &script]);
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output()
}

pub(super) fn prepare_controller_root(
    root: &Path,
    bin: &Path,
    envs: &[(String, String)],
) -> io::Result<()> {
    prepare_private_root("controller", root, bin, envs)
}

pub(super) fn prepare_observer_root(
    root: &Path,
    bin: &Path,
    envs: &[(String, String)],
) -> io::Result<()> {
    prepare_private_root("observer", root, bin, envs)
}

fn prepare_private_root(
    role: &str,
    root: &Path,
    bin: &Path,
    envs: &[(String, String)],
) -> io::Result<()> {
    let script = match role {
        "controller" => {
            r#"path="$RUNNER_TEMP/mbx-cancel-controller"
if [ -e "$path" ] || [ -L "$path" ]; then private_root_open "$path"; else private_root_create "$path"; fi
"#
        }
        "observer" => {
            r#"path="$RUNNER_TEMP/mbx-cancel-observer"
if [ -e "$path" ] || [ -L "$path" ]; then
  private_root_open "$path"
  private_child_open "$path" "$path/controller-receipt"
  private_child_open "$path" "$path/observer"
else
  private_root_create "$path"
  private_child_create "$path" "$path/controller-receipt"
  private_child_create "$path" "$path/observer"
fi
for path in "$RUNNER_TEMP/mbx-cancel-restore-parent" "$RUNNER_TEMP/mbx-cancel-restore-save"; do
  if [ -e "$path" ] || [ -L "$path" ]; then private_root_open "$path"; else private_root_create "$path"; fi
done
"#
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown root role",
            ));
        }
    };
    let output = run_bash(script, root, bin, envs)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }
}

#[test]
fn raw_api_steps_keep_multiline_source_and_scope_the_token() -> Result<(), Box<dyn Error>> {
    let env = BTreeMap::from([(
        "RUN_ID".to_owned(),
        "${{ steps.dispatch.outputs.workflow_run_id }}".to_owned(),
    )]);
    let step =
        render::token_bash_step("Dispatch", Some("dispatch"), scripts::DISPATCH, &env, None)?;
    let Yaml::Map(fields) = step else {
        return Err(io::Error::other("raw run step is not a mapping").into());
    };
    let run = fields
        .iter()
        .find(|(key, _)| key == "run")
        .map(|(_, value)| value)
        .ok_or_else(|| io::Error::other("raw run field missing"))?;
    let expected = format!(
        "{}\n{}\n{}",
        crate::schema2::mbx_stock_restore::STOCK_RESTORE_CLASSIFIER_SCRIPT,
        super::private_io::PRIVATE_IO_HELPERS,
        scripts::DISPATCH
    );
    assert_eq!(run, &Yaml::str(expected));
    let env = fields
        .iter()
        .find(|(key, _)| key == "env")
        .map(|(_, value)| value)
        .ok_or_else(|| io::Error::other("raw env field missing"))?;
    let Yaml::Map(env) = env else {
        return Err(io::Error::other("raw env is not a mapping").into());
    };
    assert!(
        env.iter().any(|(key, value)| {
            key == "GH_TOKEN" && value == &Yaml::str("${{ github.token }}")
        })
    );
    let override_token = BTreeMap::from([("GH_TOKEN".to_owned(), "bad".to_owned())]);
    assert!(render::token_bash_step("bad", None, "true", &override_token, None).is_err());
    Ok(())
}

#[test]
fn controller_artifact_download_targets_directory_and_reads_exact_file()
-> Result<(), Box<dyn Error>> {
    for (phase, artifact_name) in [
        (
            super::Phase::PreSave,
            "mbx-cancel-controller-receipt-pre-save",
        ),
        (
            super::Phase::DuringSave,
            "mbx-cancel-controller-receipt-during-save",
        ),
    ] {
        let step = super::probe_steps::download_controller_receipt_step(phase)?;
        let StepKind::Action { with, .. } = step.kind else {
            return Err(io::Error::other("receipt download is not an action").into());
        };
        assert_eq!(with.get("name").map(String::as_str), Some(artifact_name));
        assert_eq!(
            with.get("path").map(String::as_str),
            Some("${{ runner.temp }}/mbx-cancel-observer/controller-receipt")
        );
    }
    assert!(
        scripts::VALIDATE_CONTROLLER_RECEIPT
            .contains(r#"path="$RUNNER_TEMP/mbx-cancel-observer/controller-receipt/receipt.json""#)
    );
    Ok(())
}

#[test]
fn malformed_save_step_lists_never_request_step_logs() -> Result<(), Box<dyn Error>> {
    for mode in [
        "observer-duplicate-save",
        "observer-missing-save",
        "observer-object-steps",
    ] {
        let fixture = controller_fixtures::Fixture::new(mode, false)?;
        fixture.install_curl()?;
        let observer = fixture.root.join("mbx-cancel-observer/observer");
        let output = fixture.root.join("evidence.output");
        fs::write(&output, "")?;
        prepare_observer_root(&fixture.root, &fixture.bin, &fixture.env(&output, mode))?;
        fs::write(
            observer.join("cache-before.json"),
            "{\"count\":0,\"caches\":[]}\n",
        )?;
        let curl_log = fixture.root.join("curl.log");
        let summary = fixture.root.join("summary.md");
        fs::write(&summary, "")?;
        let mut env = fixture.env(&output, mode);
        env.extend([
            ("GH_TOKEN".to_owned(), "fixture-secret-token".to_owned()),
            (
                "VALIDATED_CACHE_KEY".to_owned(),
                controller_fixtures::expected_key(),
            ),
            ("CURL_LOG".to_owned(), curl_log.display().to_string()),
            ("CURL_LOCATION_MODE".to_owned(), "missing".to_owned()),
            ("CHILD_SOURCE_SHA".to_owned(), "c".repeat(40)),
            ("CHILD_ACTOR".to_owned(), "github-actions[bot]".to_owned()),
            ("CONTROLLER_CANCEL_REQUESTED".to_owned(), "true".to_owned()),
            (
                "CONTROLLER_CANCEL_AT".to_owned(),
                "2026-10-04T00:00:10Z".to_owned(),
            ),
            ("CONTROLLER_BEFORE_COUNT".to_owned(), "0".to_owned()),
            (
                "GITHUB_STEP_SUMMARY".to_owned(),
                summary.display().to_string(),
            ),
        ]);
        let script = scripts::observer_evidence();
        let result = run_bash(&script, &fixture.root, &fixture.bin, &env)?;
        assert!(
            result.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            fs::read_to_string(observer.join("child-evidence.json"))?
                .contains("\"progress_before_runner_cancel_error\":false")
        );
        let requests = if curl_log.exists() {
            fs::read_to_string(&curl_log)?
        } else {
            String::new()
        };
        assert!(
            requests.contains("child-run-api authorized=true"),
            "{mode}: {requests}"
        );
        assert!(
            requests.contains("child-jobs-api authorized=true"),
            "{mode}: {requests}"
        );
        assert!(
            !requests.contains("/steps/"),
            "{mode} fetched a step log: {requests}"
        );
        fs::remove_dir_all(fixture.root)?;
    }
    Ok(())
}
