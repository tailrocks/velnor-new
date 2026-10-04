use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{run_bash, temp_dir};
use crate::schema2::mbx_cancel_probe::scripts;

#[path = "schema2_mbx_cancel_probe_transport_fixture_tests.rs"]
mod transport;
use transport::FAKE_CURL;

const PROBE_ID: &str = "0123456789abcdef0123456789abcdef";
pub(super) const SOURCE_SHA: &str = "cccccccccccccccccccccccccccccccccccccccc";
const VICTIM_MODE: &str = "mbx-cancel-during-save-victim";
const PHASE: &str = "during-save";
const SCOPE: &str = "qualification-mbx-v1/cancel-during-save-victim";
const MBX_ACTION: &str = "jdx/mr-boxington-action@1687e54eb349cadf61fa38b5813a77875489e8e6";
const MISE_ACTION: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";
const MISE_SHA: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const RUSTC_IDENTITY: &str = "13936cde15db9d31620cb9989927cdfa06948615fc6b8291d7fba92f191a18ec";
const SCOPE_HASH: &str = "4efa592dc896090b6cfbca2010832cd66ffdbd2972b8c791100756fef657c61e";
const PRE_SCOPE_HASH: &str = "e209916d29c20a2ddbb07903ece0cca30f264f7acf4a1271efec9e57771a9d6a";

struct FixtureMode {
    victim_mode: &'static str,
    phase: &'static str,
    scope: &'static str,
    victim_name: &'static str,
    artifact_name: &'static str,
    cancel_step: &'static str,
}

fn fixture_mode(pre_save: bool) -> FixtureMode {
    if pre_save {
        FixtureMode {
            victim_mode: "mbx-cancel-pre-save-victim",
            phase: "pre-save",
            scope: "qualification-mbx-v1/cancel-pre-save-victim",
            victim_name: "MBX cancellation / pre-save victim",
            artifact_name: "mbx-cancel-victim-pre-save",
            cancel_step: "Wait at MBX pre-save cancellation point",
        }
    } else {
        FixtureMode {
            victim_mode: VICTIM_MODE,
            phase: PHASE,
            scope: SCOPE,
            victim_name: "MBX cancellation / during-save victim",
            artifact_name: "mbx-cancel-victim-during-save",
            cancel_step: "Save MBX single bundle",
        }
    }
}

#[path = "schema2_mbx_cancel_probe_date_fixture.rs"]
mod date_fixture;
#[path = "schema2_mbx_cancel_probe_controller_receipt_fixtures.rs"]
mod receipt;
pub(super) use receipt::{expected_key, readiness_receipt};

pub(super) struct Fixture {
    pub(super) root: PathBuf,
    pub(super) bin: PathBuf,
    zip: PathBuf,
    log: PathBuf,
    archive_size: u64,
    digest: String,
    pre_save: bool,
}

impl Fixture {
    pub(super) fn new(label: &str, corrupt_digest: bool) -> Result<Self, Box<dyn Error>> {
        let pre_save = label.starts_with("pre-save-");
        let root = temp_dir(label)?;
        let bin = super::fake_bin(&root)?;
        let artifact_dir = root.join("artifact");
        fs::create_dir(&artifact_dir)?;
        fs::write(
            artifact_dir.join("readiness.json"),
            readiness_receipt(pre_save),
        )?;
        let zip = root.join("victim.zip");
        let output = Command::new("zip")
            .args(["-q", "-j"])
            .arg(&zip)
            .arg("readiness.json")
            .current_dir(&artifact_dir)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other("zip fixture creation failed").into());
        }
        let archive_size = fs::metadata(&zip)?.len();
        let digest = if corrupt_digest {
            "0000000000000000000000000000000000000000000000000000000000000000".to_owned()
        } else {
            let hash = Command::new("shasum")
                .args(["-a", "256"])
                .arg(&zip)
                .output()?;
            if !hash.status.success() {
                return Err(io::Error::other("zip digest calculation failed").into());
            }
            String::from_utf8(hash.stdout)?
                .split_whitespace()
                .next()
                .ok_or_else(|| io::Error::other("zip digest missing"))?
                .to_owned()
        };
        let fixture = Self {
            log: root.join("gh.log"),
            root,
            bin,
            zip,
            digest,
            archive_size,
            pre_save,
        };
        fixture.install_curl()?;
        Ok(fixture)
    }

    pub(super) fn env(&self, output: &Path, mode: &str) -> Vec<(String, String)> {
        let phase = fixture_mode(self.pre_save);
        let env = BTreeMap::from([
            (
                "GITHUB_REPOSITORY".to_owned(),
                "tailrocks/velnor-new".to_owned(),
            ),
            (
                "GITHUB_EVENT_NAME".to_owned(),
                "workflow_dispatch".to_owned(),
            ),
            ("GITHUB_REF".to_owned(), "refs/heads/main".to_owned()),
            (
                "GITHUB_WORKFLOW_REF".to_owned(),
                "tailrocks/velnor-new/.github/workflows/qualification.yml@refs/heads/main"
                    .to_owned(),
            ),
            ("GITHUB_SHA".to_owned(), SOURCE_SHA.to_owned()),
            ("GITHUB_RUN_ID".to_owned(), "900".to_owned()),
            ("GITHUB_RUN_ATTEMPT".to_owned(), "1".to_owned()),
            ("GITHUB_ACTOR".to_owned(), "fixture-controller".to_owned()),
            ("GITHUB_OUTPUT".to_owned(), output.display().to_string()),
            ("RUNNER_TEMP".to_owned(), self.root.display().to_string()),
            ("REF_PROTECTED".to_owned(), "true".to_owned()),
            ("PROBE_ID".to_owned(), PROBE_ID.to_owned()),
            ("VICTIM_MODE".to_owned(), phase.victim_mode.to_owned()),
            (
                "CONTROLLER_MODE".to_owned(),
                format!("mbx-cancel-{}-controller", phase.phase),
            ),
            ("PROBE_PHASE".to_owned(), phase.phase.to_owned()),
            ("CACHE_SCOPE".to_owned(), phase.scope.to_owned()),
            ("MBX_GENERATION".to_owned(), "velnor-mbx-1.22.0".to_owned()),
            ("MBX_VERSION".to_owned(), "1.22.0".to_owned()),
            ("MBX_ACTION_USES".to_owned(), MBX_ACTION.to_owned()),
            ("RUST_VERSION".to_owned(), "1.98.1".to_owned()),
            ("MISE_ACTION_USES".to_owned(), MISE_ACTION.to_owned()),
            ("MISE_VERSION".to_owned(), "2025.9.5".to_owned()),
            ("MISE_SHA256".to_owned(), MISE_SHA.to_owned()),
            ("VICTIM_JOB_NAME".to_owned(), phase.victim_name.to_owned()),
            (
                "VICTIM_ARTIFACT_NAME".to_owned(),
                phase.artifact_name.to_owned(),
            ),
            ("CANCEL_STEP_NAME".to_owned(), phase.cancel_step.to_owned()),
            ("RUN_ID".to_owned(), "123".to_owned()),
            ("WORKFLOW_ID".to_owned(), "77".to_owned()),
            ("GH_LOG".to_owned(), self.log.display().to_string()),
            (
                "GH_STATE".to_owned(),
                self.root.join("gh-state").display().to_string(),
            ),
            (
                "CURL_LOG".to_owned(),
                self.root.join("curl.log").display().to_string(),
            ),
            ("GH_TOKEN".to_owned(), "fixture-secret-token".to_owned()),
            ("GH_ARTIFACT_ZIP".to_owned(), self.zip.display().to_string()),
            ("GH_ARTIFACT_SIZE".to_owned(), self.archive_size.to_string()),
            ("GH_ARTIFACT_DIGEST".to_owned(), self.digest.clone()),
            ("GH_MODE".to_owned(), mode.to_owned()),
        ]);
        env.into_iter().collect()
    }

    pub(super) fn output(&self, label: &str) -> PathBuf {
        self.root.join(format!("{label}.output"))
    }

    pub(super) fn install_curl(&self) -> io::Result<()> {
        let curl = self.bin.join("curl");
        fs::write(&curl, FAKE_CURL)?;
        fs::set_permissions(curl, fs::Permissions::from_mode(0o755))
    }

    pub(super) fn install_date(&self) -> io::Result<()> {
        date_fixture::install(&self.bin)
    }

    pub(super) fn log(&self) -> Result<String, Box<dyn Error>> {
        Ok(fs::read_to_string(&self.log)?)
    }

    pub(super) fn dispatch(&self, mode: &str) -> Result<(PathBuf, String), Box<dyn Error>> {
        let output = self.output("dispatch");
        fs::write(&output, "")?;
        let result = run_bash(
            scripts::DISPATCH,
            &self.root,
            &self.bin,
            &self.env(&output, mode),
        )?;
        if !result.status.success() {
            return Err(io::Error::other(String::from_utf8_lossy(&result.stderr)).into());
        }
        Ok((output.clone(), fs::read_to_string(output)?))
    }

    pub(super) fn readiness(&self, mode: &str) -> Result<String, Box<dyn Error>> {
        self.readiness_with_run(mode, "123")
    }

    pub(super) fn readiness_with_run(
        &self,
        mode: &str,
        run_id: &str,
    ) -> Result<String, Box<dyn Error>> {
        let output = self.output("readiness");
        fs::write(&output, "")?;
        let mut env = self.env(&output, mode);
        set_env(&mut env, "RUN_ID", run_id);
        let result = run_bash(&scripts::wait_readiness(), &self.root, &self.bin, &env)?;
        if !result.status.success() {
            return Err(io::Error::other(String::from_utf8_lossy(&result.stderr)).into());
        }
        Ok(fs::read_to_string(output)?)
    }

    pub(super) fn cancel(&self, mode: &str) -> Result<String, Box<dyn Error>> {
        self.cancel_with_run(mode, "123")
    }

    pub(super) fn cancel_with_run(
        &self,
        mode: &str,
        run_id: &str,
    ) -> Result<String, Box<dyn Error>> {
        let output = self.output("cancel");
        fs::write(&output, "")?;
        let mut env = self.env(&output, mode);
        set_env(&mut env, "RUN_ID", run_id);
        let result = run_bash(&scripts::cancel_exact(), &self.root, &self.bin, &env)?;
        if !result.status.success() {
            return Err(io::Error::other(String::from_utf8_lossy(&result.stderr)).into());
        }
        Ok(fs::read_to_string(output)?)
    }
}

fn set_env(env: &mut [(String, String)], name: &str, value: &str) {
    if let Some((_, current)) = env.iter_mut().find(|(key, _)| key == name) {
        current.clear();
        current.push_str(value);
    }
}

#[test]
fn controller_dispatches_and_cancels_only_the_returned_validated_run() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::new("controller-good", false)?;
    let (_, dispatch) = fixture.dispatch("good")?;
    assert!(dispatch.contains("dispatch_status=200\n"));
    assert!(dispatch.contains("workflow_id=77\n"));
    assert!(dispatch.contains("workflow_run_id=123\n"));
    assert!(
        dispatch.contains(
            "run_url=https://api.github.com/repos/tailrocks/velnor-new/actions/runs/123\n"
        )
    );
    let ready = fixture.readiness("good")?;
    assert!(ready.contains("ready=true\n"), "{ready}");
    assert!(ready.contains("reason=exact_identity_and_cancel_window_ready\n"));
    let cancel = fixture.cancel("good")?;
    assert!(cancel.contains("cancel_requested=true\n"), "{cancel}");
    assert!(cancel.contains("cancel_status=202\n"));
    assert!(cancel.contains("post_revalidated=true\n"));
    let log = fixture.log()?;
    assert!(log.contains(
        "POST /repos/tailrocks/velnor-new/actions/workflows/qualification.yml/dispatches"
    ));
    assert!(log.contains("GET /repos/tailrocks/velnor-new/actions/runs/123\n"));
    assert!(log.contains("POST /repos/tailrocks/velnor-new/actions/runs/123/cancel\n"));
    assert!(!log.contains("GET /repos/tailrocks/velnor-new/actions/runs?"));
    assert!(!log.contains("/actions/caches/"));
    let curl_log = fs::read_to_string(fixture.root.join("curl.log"))?;
    assert_eq!(
        curl_log.lines().collect::<Vec<_>>(),
        [
            "artifact-api authorized=true",
            "artifact-signed authorized=false",
            "artifact-api authorized=true",
            "artifact-signed authorized=false",
        ]
    );
    assert!(!curl_log.contains("fixture-secret-token"));
    assert!(!curl_log.contains("sig=fixture-only"));
    assert!(
        !fixture
            .root
            .join("mbx-cancel-controller/artifact.curlrc")
            .exists()
    );
    assert!(
        !fixture
            .root
            .join("mbx-cancel-controller/artifact-response.headers")
            .exists()
    );
    fs::remove_dir_all(fixture.root)?;
    Ok(())
}

#[test]
fn malformed_dispatch_or_victim_identity_never_reaches_cancel() -> Result<(), Box<dyn Error>> {
    for mode in ["missing-id", "bad-url", "initial-mismatch"] {
        let fixture = Fixture::new(&format!("controller-{mode}"), false)?;
        let (_, dispatch) = fixture.dispatch(mode)?;
        assert!(
            dispatch.contains("dispatch_status=200\n"),
            "{mode}: {dispatch}"
        );
        let run_id = dispatch
            .lines()
            .find_map(|line| line.strip_prefix("workflow_run_id="))
            .ok_or_else(|| io::Error::other("dispatch run ID output missing"))?;
        assert!(run_id.is_empty(), "{mode}: {dispatch}");
        let ready = fixture.readiness_with_run(mode, run_id)?;
        assert!(ready.contains("ready=false\n"), "{mode}: {ready}");
        let cancel = fixture.cancel_with_run(mode, run_id)?;
        assert!(
            cancel.contains("cancel_requested=false\n"),
            "{mode}: {cancel}"
        );
        assert!(!fixture.log()?.contains("/actions/runs/123/cancel"));
        let run_queries = fixture
            .log()?
            .lines()
            .filter(|line| *line == "GET /repos/tailrocks/velnor-new/actions/runs/123")
            .count();
        assert_eq!(
            run_queries,
            usize::from(mode == "initial-mismatch"),
            "{mode}"
        );
        fs::remove_dir_all(fixture.root)?;
    }

    let mismatch = Fixture::new("controller-run-mismatch", false)?;
    mismatch.dispatch("mismatch")?;
    let ready = mismatch.readiness("mismatch")?;
    assert!(ready.contains("ready=false\n"));
    assert!(ready.contains("reason=exact_run_identity_mismatch\n"));
    let cancel = mismatch.cancel("mismatch")?;
    assert!(cancel.contains("cancel_requested=false\n"));
    assert!(!mismatch.log()?.contains("/actions/runs/123/cancel"));

    let bad_artifact = Fixture::new("controller-bad-digest", true)?;
    bad_artifact.dispatch("good")?;
    let ready = bad_artifact.readiness("good")?;
    assert!(ready.contains("ready=false\n"));
    assert!(ready.contains("reason=victim_receipt_invalid\n"));
    let cancel = bad_artifact.cancel("good")?;
    assert!(cancel.contains("cancel_requested=false\n"));
    assert!(!bad_artifact.log()?.contains("/actions/runs/123/cancel"));

    fs::remove_dir_all(mismatch.root)?;
    fs::remove_dir_all(bad_artifact.root)?;
    Ok(())
}
