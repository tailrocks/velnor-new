// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::image::{ImageAttestationIdentity, verify_image_file_attestation};
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use velnor_actions_mise::CheckDeadline;

#[path = "../../velnor-actions-cli/tests/api2026_pull_context_fixture/non_inputs.rs"]
mod non_inputs;
#[path = "../../velnor-actions-cli/tests/api2026_pull_context_fixture/worker.rs"]
mod worker;

const TEST: &str = crate::libtest_case::name(concat!(
    module_path!(),
    "::real_github_transport_verifies_the_signed_record"
));
const SOURCE: &str = "a3417b150ac2f477498cadc329cdf4e23aa5b54b";
const WORKFLOW: &str = ".github/workflows/job-image-candidate.yml";
const HISTORICAL_TEST: &str = crate::libtest_case::name(concat!(
    module_path!(),
    "::historical_release_assets_use_pinned_github_transport"
));

#[test]
fn same_base_history_rejects_forged_tool() -> Result<(), Box<dyn Error>> {
    const SCENARIO: &str = crate::libtest_case::name(concat!(
        module_path!(),
        "::same_base_history_rejects_forged_tool"
    ));
    if std::env::var_os("VELNOR_GH_FIXTURE_WORKER").is_none() {
        return dispatch(SCENARIO);
    }
    worker::validate_worker("image_attestation", Some(SCENARIO))?;
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/attestation_fixtures");
    let state = tempfile::tempdir()?;
    let _server = Server::start_with(
        state.path(),
        &fixture,
        include_str!("../tests/attestation_fixtures/historical-assets-transport.py"),
    )?;
    for forged in [false, true] {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args([
                "--exact",
                "release::crate_release_runtime::tests::test_only_dispatch_and_loopback_transport",
            ])
            .env("VELNOR_TEST_SAME_BASE_SIGNED", "1")
            .env("VELNOR_TEST_PINNED_GIT", "/tools/pinned-git")
            .env(
                "VELNOR_TEST_PINNED_GIT_HTTP",
                "/tools/pinned-git-remote-http",
            );
        if forged {
            command.env("VELNOR_TEST_FORGED_TOOL", "1");
        }
        let output = command.output()?;
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        crate::libtest_case::assert_one_passed(&output);
    }
    worker::record_success("image_attestation", SCENARIO)?;
    Ok(())
}

#[test]
fn signed_controller_dispatch_rechecks_historical_authority() -> Result<(), Box<dyn Error>> {
    const SCENARIO: &str = crate::libtest_case::name(concat!(
        module_path!(),
        "::signed_controller_dispatch_rechecks_historical_authority"
    ));
    if std::env::var_os("VELNOR_GH_FIXTURE_WORKER").is_none() {
        return dispatch(SCENARIO);
    }
    worker::validate_worker("image_attestation", Some(SCENARIO))?;
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/attestation_fixtures");
    let state = tempfile::tempdir()?;
    let _server = Server::start_with(
        state.path(),
        &fixture,
        include_str!("../tests/attestation_fixtures/historical-assets-transport.py"),
    )?;
    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "release::crate_release_runtime::tests::test_only_dispatch_and_loopback_transport",
        ])
        .env("VELNOR_TEST_SAME_BASE_SIGNED", "1")
        .env("VELNOR_TEST_CONTROLLER_SCENARIO", "1")
        .env("VELNOR_TEST_PINNED_GIT", "/tools/pinned-git")
        .env(
            "VELNOR_TEST_PINNED_GIT_HTTP",
            "/tools/pinned-git-remote-http",
        )
        .output()?;
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    crate::libtest_case::assert_one_passed(&output);
    worker::record_success("image_attestation", SCENARIO)?;
    Ok(())
}

#[test]
fn historical_release_assets_use_pinned_github_transport() -> Result<(), Box<dyn Error>> {
    if std::env::var_os("VELNOR_GH_FIXTURE_WORKER").is_none() {
        return dispatch(HISTORICAL_TEST);
    }
    worker::validate_worker("image_attestation", Some(HISTORICAL_TEST))?;
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/attestation_fixtures");
    let state = tempfile::tempdir()?;
    let _server = Server::start_with(
        state.path(),
        &fixture,
        include_str!("../tests/attestation_fixtures/historical-assets-transport.py"),
    )?;
    let run = || {
        crate::release::package_release_publish::read_historical_sdk_attestation_assets(
            "0.1.5",
            "f1041f322c54cd7edfdef06afd307799eeaa801a",
            "e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52",
            CheckDeadline::after(Duration::from_secs(90)).expect("deadline"),
        )
    };
    let result = run()?;
    assert_eq!(result.0, fs::read(fixture.join("release-manifest.json"))?);
    assert_eq!(
        result.1,
        fs::read(fixture.join("release-manifest.json.intoto.jsonl"))?
    );
    for mode in ["missing", "duplicate", "digest", "size", "moved-tag"] {
        fs::write(state.path().join("mode"), mode)?;
        assert!(run().is_err(), "{mode} accepted");
    }
    worker::record_success("image_attestation", HISTORICAL_TEST)?;
    Ok(())
}

#[test]
fn real_github_transport_verifies_the_signed_record() -> Result<(), Box<dyn Error>> {
    if std::env::var_os("VELNOR_GH_FIXTURE_WORKER").is_none() {
        return dispatch(TEST);
    }
    worker::validate_worker("image_attestation", Some(TEST))?;
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/attestation_fixtures/image-job");
    let state = tempfile::tempdir()?;
    let _server = Server::start(
        state.path(),
        &fixture.join("job-image-candidate.json.intoto.jsonl"),
    )?;
    let mut identity = ImageAttestationIdentity {
        repository: "tailrocks/velnor-new",
        repository_id: 1_390_620_900,
        caller_workflow_path: WORKFLOW,
        caller_workflow_sha: SOURCE,
        signer_workflow_path: WORKFLOW,
        signer_workflow_sha: SOURCE,
        signer_ref: "refs/heads/main",
        source_sha: SOURCE,
        source_ref: "refs/heads/main",
        event: "push",
        run_id: 42_149_229_551,
        run_attempt: 1,
    };
    let subjects = BTreeMap::from([
        (
            "job-image.oci.tar".to_owned(),
            "621cb823d7e0d7edaf4e307da48fa7650b62b7d4a0796584489301110286f037".to_owned(),
        ),
        (
            "job-image-candidate.json".to_owned(),
            "41b0c6b72052e4a7f6532180112a7aa9129de552ae29b560931445fcca7844b2".to_owned(),
        ),
        (
            "job-image-native-source-closure.json".to_owned(),
            "4c4a70ff8d97094ac9d79a46391fa1c908709768ab5335fd43e53f5b5e2e6830".to_owned(),
        ),
    ]);
    let record = fixture.join("job-image-candidate.json");
    let deadline = CheckDeadline::after(Duration::from_secs(90))?;
    verify_image_file_attestation(&record, &identity, &subjects, &[], deadline)?;
    identity.source_sha = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    assert!(verify_image_file_attestation(&record, &identity, &subjects, &[], deadline).is_err());
    identity.source_sha = SOURCE;
    fs::write(state.path().join("missing"), b"missing")?;
    assert!(verify_image_file_attestation(&record, &identity, &subjects, &[], deadline).is_err());
    worker::record_success("image_attestation", TEST)?;
    Ok(())
}

pub(crate) fn dispatch(scenario: &str) -> Result<(), Box<dyn Error>> {
    let source = non_inputs::prepare_source()?;
    let status = source
        .command()
        .args([
            "image_attestation",
            scenario,
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
        ])
        .status()?;
    if !status.success() {
        return Err(format!("isolated image attestation fixture failed: {status}").into());
    }
    Ok(())
}

pub(crate) fn validate(scenario: &str) -> Result<(), Box<dyn Error>> {
    worker::validate_worker("image_attestation", Some(scenario))
}

pub(super) fn validate_probe_context(scenario: &str) -> Result<(), Box<dyn Error>> {
    worker::validate_worker_with_nonce("image_attestation", Some(scenario), Some(&"a".repeat(64)))
}

pub(crate) fn record_success(scenario: &str) -> Result<(), Box<dyn Error>> {
    worker::record_success("image_attestation", scenario)
}

struct Server {
    child: Child,
    _lock: fs::File,
}

impl Server {
    fn start(state: &std::path::Path, bundle: &std::path::Path) -> Result<Self, Box<dyn Error>> {
        Self::start_with(
            state,
            bundle,
            include_str!("../tests/attestation_fixtures/image-job/transport.py"),
        )
    }

    fn start_with(
        state: &std::path::Path,
        bundle: &std::path::Path,
        script_source: &str,
    ) -> Result<Self, Box<dyn Error>> {
        let tls = std::env::var_os("VELNOR_GH_TEST_TLS_ROOT").ok_or("missing TLS fixture")?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open("/tmp/velnor-gh-fixture.lock")?;
        lock.lock()?;
        let script = state.join("transport.py");
        fs::write(&script, script_source)?;
        let mut child = Command::new("/usr/bin/python3")
            .args([
                script.as_os_str(),
                state.as_os_str(),
                &tls,
                bundle.as_os_str(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while !state.join("ready").exists() {
            if let Some(status) = child.try_wait()? {
                return Err(format!("attestation fixture exited: {status}").into());
            }
            if Instant::now() >= deadline {
                drop(child.kill());
                drop(child.wait());
                return Err("attestation fixture startup timed out".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(Self { child, _lock: lock })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        drop(self.child.kill());
        drop(self.child.wait());
    }
}
