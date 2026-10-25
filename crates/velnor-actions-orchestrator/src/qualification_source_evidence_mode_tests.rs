//! File-mode checks for source evidence.

use std::env;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::process::Command;

use velnor_actions_contract::workflow::QualificationCacheProducerContext;

use super::{JOB_ID, RUN_ID, RUN_KEY, fixture, write_reports};
use crate::qualification_cache_runtime::closure::ObservedLaneClosure;
use crate::qualification_source_evidence::context::AuthenticatedProducerContext;
use crate::qualification_source_evidence::reports::read_successful_lane_reports;
use crate::qualification_source_evidence::{SOURCE_EVIDENCE_FILENAME, write_sidecar};

const CHILD_UMASK_TEST: &str = "VELNOR_SOURCE_EVIDENCE_CHILD_UMASK_TEST";

#[test]
fn source_sidecar_is_private_under_child_umask_0022() {
    if env::var_os(CHILD_UMASK_TEST).is_some() {
        write_and_check_modes();
        return;
    }

    let executable = env::current_exe().expect("test executable");
    let test_name = format!(
        "{}::source_sidecar_is_private_under_child_umask_0022",
        module_path!()
    );
    let status = Command::new("sh")
        .args(["-c", "umask 0022; exec \"$@\"", "sh"])
        .arg(executable)
        .args(["--exact", &test_name])
        .env(CHILD_UMASK_TEST, "1")
        .status()
        .expect("start child test process");
    assert!(status.success(), "child test exited with {status}");
}

fn write_and_check_modes() {
    let (temp, plan, entry, obligation) = fixture();
    write_reports(&temp, &plan, &entry, 0);
    let reports = read_successful_lane_reports(temp.path(), &plan, &entry, &entry.task_digest)
        .expect("successful reports");
    let producer = QualificationCacheProducerContext {
        mode: plan.qualification.as_ref().expect("dispatch").mode,
        repository: "owner/repo".to_owned(),
        default_branch: "main".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        ref_protected: true,
        workflow_ref: "owner/repo/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        workflow_sha: plan.head.clone(),
        source_sha: plan.head.clone(),
        run: velnor_actions_contract::workflow::QualificationRunRef {
            run_id: RUN_ID,
            run_attempt: 1,
        },
    };
    let authenticated = AuthenticatedProducerContext {
        dispatch: plan.qualification.clone().expect("dispatch"),
        producer,
        runner_temp: temp.path().to_path_buf(),
        job_id: JOB_ID.to_owned(),
        run_key: RUN_KEY.to_owned(),
    };
    let observed = ObservedLaneClosure {
        matrix_key: entry.matrix_key.clone(),
        task_id: entry.task_id.clone(),
        observed_task_digest: entry.task_digest.clone(),
        observed_input_digest: entry.input_digest.clone(),
        observed_closure_digest: obligation.closure_digest,
    };

    write_sidecar(
        temp.path(),
        &plan,
        &entry,
        &authenticated,
        &plan.generator,
        &observed,
        reports,
    )
    .expect("write source evidence file");

    let sidecar = temp
        .path()
        .join("velnor")
        .join(RUN_KEY)
        .join(&entry.matrix_key)
        .join(SOURCE_EVIDENCE_FILENAME);
    assert_eq!(
        fs::metadata(sidecar)
            .expect("source evidence file metadata")
            .mode()
            & 0o777,
        0o600
    );

    let ordinary = temp.path().join("ordinary-producer-file");
    crate::exclusive_write::write_exclusive(&ordinary, b"ordinary", "ordinary")
        .expect("write ordinary producer file");
    assert_eq!(
        fs::metadata(ordinary)
            .expect("ordinary file metadata")
            .mode()
            & 0o777,
        0o644
    );
}
