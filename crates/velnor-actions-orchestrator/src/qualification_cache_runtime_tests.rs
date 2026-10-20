use super::files::{
    GUEST_IMAGE_PROJECTION_SCHEMA, GuestImageDigests, GuestImageProjection, MAX_PLAN_JSON_BYTES,
};
use super::*;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use velnor_actions_contract::workflow::{
    EntryCacheIds, ExecuteTaskIds, MatrixEntry, ObligationDecision, Plan, PlanGenerator,
    PlanMatrix, PlanObligation, PlanPackage, PlanRunner, PlannedPlatform,
    QualificationCacheDirective, QualificationDispatch, QualificationPhase, QualificationRunRef,
    QualificationRuntimeAbiObservation, QualificationRuntimeIdentity,
    QualificationRuntimeIdentityRequirements, QualificationRuntimeImageDigests,
    QualificationRuntimeImageIdentity, QualificationRuntimePlatform, QualificationRuntimeScaleSet,
    QualificationRuntimeToolObservation, Trust, WorkflowEvent,
};
use velnor_actions_contract::{
    RunnerSelection, ScaleSetSelector,
    cachekey::{ToolchainInputs, toolchain_id},
    canonical_json_str, digest_b3, plan_id_for_run, run_key_for_ci,
};

const SOURCE: &str = "0123456789abcdef0123456789abcdef01234567";
const RUN_ID: u64 = 700;
const REPOSITORY: &str = "owner/project";
const WORKFLOW_REF: &str = "owner/project/.github/workflows/ci.yml@refs/heads/main";

#[test]
fn root_observation_is_written_and_read_from_the_pinned_owner_run_directory() {
    let fixture = Fixture::cold();
    let context = fixture.plan.qualification.as_ref().expect("context");
    let path = write_qualification_cache_roots(
        fixture.runner_temp.path(),
        context.run_id,
        context.run_attempt,
        &fixture.owner_id,
        &fixture.layer,
        |name| match name {
            "CARGO_HOME" => Some(
                fixture
                    .runner_temp
                    .path()
                    .join("velnor/cargo")
                    .display()
                    .to_string(),
            ),
            _ => None,
        },
    )
    .expect("write root record");
    let text = read_qualification_cache_roots(&fixture.request(), Path::new(&path))
        .expect("read pinned root record");
    let value: serde_json::Value = serde_json::from_str(&text).expect("root JSON");
    assert_eq!(value["owner_id"], fixture.owner_id);
    assert_eq!(value["layer"], fixture.layer);
    assert_eq!(value["roots"].as_array().expect("roots").len(), 3);
    assert_eq!(
        fs::metadata(path)
            .expect("root file metadata")
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
}

#[path = "qualification_cache_runtime_test_fixture.rs"]
mod fixture;
use fixture::*;

fn bind(request: &QualificationCacheBindingRequest<'_>) -> QualificationCacheBindingOutputs {
    bind_qualification_cache_runtime(request).expect("cache binding")
}

fn bind_result(
    request: &QualificationCacheBindingRequest<'_>,
) -> Result<QualificationCacheBindingOutputs, OrchestratorError> {
    bind_qualification_cache_runtime(request)
}

#[path = "qualification_cache_runtime_cases_tests.rs"]
mod cases;
