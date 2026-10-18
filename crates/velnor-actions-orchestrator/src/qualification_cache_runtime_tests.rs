use super::files::{
    GUEST_IMAGE_PROJECTION_SCHEMA, GuestImageDigests, GuestImageProjection, MAX_PLAN_JSON_BYTES,
};
use super::*;

use std::fs;
use std::sync::mpsc;
use std::time::Duration;

use velnor_actions_contract::workflow::{
    EntryCacheIds, ExecuteTaskIds, MatrixEntry, ObligationDecision, Plan, PlanBaseline,
    PlanGenerator, PlanMatrix, PlanObligation, PlanPackage, PlanRunner, PlannedPlatform,
    QualificationDispatch, QualificationPhase, QualificationRunRef,
    QualificationRuntimeAbiObservation, QualificationRuntimeIdentityRequirements,
    QualificationRuntimeImageDigests, QualificationRuntimeImageIdentity,
    QualificationRuntimePlatform, QualificationRuntimeScaleSet,
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
fn cold_binding_emits_one_exact_restore_and_save_key() {
    let fixture = Fixture::cold();
    let request = fixture.request();
    let outputs = bind(&request);

    assert!(outputs.active);
    assert_eq!(outputs.layer, "cargo_sources");
    assert_eq!(outputs.phase, "cold");
    assert_eq!(outputs.expected_cache, None);
    assert!(
        outputs
            .restore_key
            .as_deref()
            .is_some_and(|key| key.ends_with("-k1"))
    );
    assert_eq!(outputs.save_key, outputs.restore_key);
    assert!(!outputs.save_if_state_changes);
}

#[test]
fn plan_artifact_uses_the_four_mib_plan_limit() {
    let fixture = Fixture::cold();
    let mut plan = fixture.plan.clone();
    plan.warnings = vec!["large-plan-warning".repeat(2_048); 4];
    let bytes = serde_json::to_vec(&plan).expect("large plan JSON");
    assert!(bytes.len() > MAX_RUNTIME_IDENTITY_JSON_BYTES);
    assert!(
        bytes.len()
            < usize::try_from(MAX_PLAN_JSON_BYTES).expect("plan byte limit fits this target")
    );
    let run_dir = fixture
        .runner_temp
        .path()
        .join("velnor")
        .join(&plan.run_key);
    fs::write(run_dir.join("plan.json"), bytes).expect("large plan artifact");

    assert!(bind_result(&fixture.request()).is_ok());

    let plan_limit = usize::try_from(MAX_PLAN_JSON_BYTES).expect("plan limit");
    plan.warnings = vec!["x".repeat(plan_limit)];
    let oversized = serde_json::to_vec(&plan).expect("oversized plan JSON");
    fs::write(run_dir.join("plan.json"), oversized).expect("oversized plan artifact");
    assert!(bind_result(&fixture.request()).is_err());
}

#[test]
fn malformed_duplicate_and_stale_directives_fail_closed() {
    let fixture = Fixture::cold();
    let request = fixture.request();
    fixture.write_directive(r#"{"schema":2,"schema":2}"#);
    assert!(bind_result(&request).is_err());

    let mut stale: QualificationCacheDirective =
        serde_json::from_str(&fixture.directive_json).expect("directive");
    stale.campaign = "other-campaign".to_owned();
    let stale_json = canonical_json_str(&stale).expect("canonical directive");
    fixture.write_directive(&stale_json);
    assert!(bind_result(&request).is_err());

    let mut unknown: serde_json::Value =
        serde_json::from_str(&fixture.directive_json).expect("directive value");
    unknown["future"] = serde_json::json!(true);
    let unknown_json = canonical_json_str(&unknown).expect("canonical directive value");
    fixture.write_directive(&unknown_json);
    assert!(bind_result(&request).is_err());
}

#[test]
fn large_valid_directive_is_read_from_the_run_artifact() {
    let fixture = Fixture::cold();
    let mut plan = fixture.plan.clone();
    let template = plan.matrix.include[0].clone();
    for index in 0..64 {
        let task_id = format!("stack/rust/demo/test/cache-lane-{index:03}");
        let mut entry = MatrixEntry::derive(
            "rust",
            &task_id,
            "mise run test",
            &template.task_digest,
            template.adapter_metadata.clone(),
            ExecuteTaskIds::default(),
            &template.input_digest,
            &plan.run_key,
            "rust-demo",
            template.planned_platform.clone(),
        )
        .expect("cache-test lane");
        entry.cache_ids = template.cache_ids.clone();
        entry.toolchain_inputs = template.toolchain_inputs.clone();
        entry.toolchain_id = template.toolchain_id.clone();
        plan.matrix.include.push(entry);
    }
    plan.matrix
        .include
        .sort_by(|left, right| left.id.cmp(&right.id));
    plan.validate().expect("expanded plan");
    let directive = QualificationCacheDirective::for_plan(&plan, None)
        .expect("directive for expanded plan")
        .expect("qualification directive");
    let directive_json = canonical_json_str(&directive).expect("directive JSON");
    assert!(directive_json.len() > MAX_RUNTIME_IDENTITY_JSON_BYTES);

    let run_dir = fixture
        .runner_temp
        .path()
        .join("velnor")
        .join(&plan.run_key);
    fs::write(
        run_dir.join("plan.json"),
        serde_json::to_vec(&plan).expect("plan JSON"),
    )
    .expect("expanded plan artifact");
    fs::write(
        run_dir.join(QUALIFICATION_CACHE_DIRECTIVE_FILENAME),
        &directive_json,
    )
    .expect("directive artifact");

    assert!(bind_result(&fixture.request()).is_ok());
}

#[test]
fn dispatch_source_ref_and_attempt_mismatches_fail_closed() {
    let fixture = Fixture::cold();

    let mut request = fixture.request();
    request.source_sha = "ffffffffffffffffffffffffffffffffffffffff";
    assert!(bind_result(&request).is_err());

    let mut request = fixture.request();
    request.git_ref = "refs/heads/other";
    assert!(bind_result(&request).is_err());

    let mut request = fixture.request();
    request.run_attempt = 2;
    assert!(bind_result(&request).is_err());

    let mut request = fixture.request();
    request.repository = "other/project";
    assert!(bind_result(&request).is_err());

    let mut request = fixture.request();
    request.workflow_ref = "other/project/.github/workflows/ci.yml@refs/heads/main";
    assert!(bind_result(&request).is_err());
}

#[test]
fn trusted_plan_and_symlink_plan_fail_closed() {
    let fixture = Fixture::cold();
    let mut plan = fixture.plan.clone();
    plan.trust = Trust::Trusted;
    fixture.write_plan(&plan);
    assert!(bind_result(&fixture.request()).is_err());

    let plan = fixture.plan.clone();
    let run_dir = fixture
        .runner_temp
        .path()
        .join("velnor")
        .join(&plan.run_key);
    let plan_path = run_dir.join("plan.json");
    let target = fixture.runner_temp.path().join("plan-target.json");
    fs::write(&target, serde_json::to_vec(&plan).expect("plan JSON")).expect("target plan");
    fs::remove_file(&plan_path).expect("remove plan");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &plan_path).expect("symlink plan");
    assert!(bind_result(&fixture.request()).is_err());
}

#[cfg(unix)]
#[test]
fn symlink_directive_fails_closed() {
    let fixture = Fixture::cold();
    let run_dir = fixture
        .runner_temp
        .path()
        .join("velnor")
        .join(&fixture.plan.run_key);
    let directive_path = run_dir.join(QUALIFICATION_CACHE_DIRECTIVE_FILENAME);
    let target = fixture.runner_temp.path().join("directive-target.json");
    fs::write(&target, &fixture.directive_json).expect("directive target");
    fs::remove_file(&directive_path).expect("remove directive");
    std::os::unix::fs::symlink(&target, &directive_path).expect("directive symlink");

    assert!(bind_result(&fixture.request()).is_err());
}

#[test]
fn missing_warm_predecessor_artifact_fails_closed() {
    let fixture = Fixture::warm_without_admission();
    let request = fixture.request();
    assert!(bind_result(&request).is_err());
}

#[test]
fn scale_set_without_configured_runner_image_source_fails_before_projection_read() {
    let fixture = Fixture::cold();
    let mut plan = fixture.plan.clone();
    let selector = ScaleSetSelector::try_new(
        "ubuntu-26.04-scale-set",
        &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
    )
    .expect("Scale Set selector");
    let planned = PlannedPlatform::new(&selector.token(), "x86_64-unknown-linux-gnu")
        .expect("Scale Set platform");
    let entry = &mut plan.matrix.include[0];
    entry.planned_platform = planned.clone();
    let toolchain_inputs = entry
        .toolchain_inputs
        .as_ref()
        .expect("plan toolchain inputs");
    entry.cache_ids = Some(
        EntryCacheIds::new(
            &digest_b3(b"workspace"),
            &digest_b3(b"lane"),
            &planned.platform_id,
            &toolchain_id(toolchain_inputs).expect("toolchain identity"),
            &digest_b3(b"cache format"),
        )
        .expect("cache identity"),
    );
    fixture.write_plan(&plan);

    let error = bind_result(&fixture.request()).expect_err("Scale Set image source is absent");
    assert!(
        error
            .to_string()
            .contains("qualification_guest_image_source_authority_missing"),
        "unexpected error: {error}"
    );
}

#[test]
fn guest_projection_uses_runner_image_source_separate_from_consumer_source() {
    let consumer_source = "a".repeat(40);
    let runner_image_source = "b".repeat(40);
    let selector = ScaleSetSelector::try_new(
        "ubuntu-26.04-scale-set",
        &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
    )
    .expect("Scale Set selector");
    let image = || QualificationRuntimeImageDigests {
        archive_sha256: "1".repeat(64),
        archive_index_digest: "2".repeat(64),
        image_index_digest: None,
        manifest_digest: format!("sha256:{}", "3".repeat(64)),
        config_digest: format!("sha256:{}", "4".repeat(64)),
    };
    let projection = GuestImageProjection {
        schema: GUEST_IMAGE_PROJECTION_SCHEMA.to_owned(),
        source_sha: runner_image_source.clone(),
        guest_platform: "linux/amd64".to_owned(),
        scale_set: QualificationRuntimeScaleSet {
            name: selector.name().to_owned(),
            labels: selector.labels().to_vec(),
        },
        images: GuestImageDigests {
            runner: image(),
            dind: image(),
        },
    };

    let identity = verified_scale_set_image_identity(&projection, &runner_image_source, &selector)
        .expect("configured runner image source matches projection");
    let QualificationRuntimeImageIdentity::ScaleSet { source_sha, .. } = identity else {
        panic!("Scale Set projection creates Scale Set identity");
    };
    assert_eq!(source_sha, runner_image_source);
    assert_ne!(source_sha, consumer_source);
    assert!(verified_scale_set_image_identity(&projection, &consumer_source, &selector).is_err());
}

#[test]
fn unknown_matrix_key_and_unknown_layer_fail_closed() {
    let fixture = Fixture::cold();
    let mut request = fixture.request();
    request.matrix_key = "matrix-unknown";
    assert!(bind_result(&request).is_err());

    let mut request = fixture.request();
    request.layer = "future_layer";
    assert!(bind_result(&request).is_err());
}

struct Fixture {
    runner_temp: tempfile::TempDir,
    plan: Plan,
    directive_json: String,
    runtime_identity_json: String,
    matrix_key: String,
    layer: String,
}

impl Fixture {
    fn cold() -> Self {
        Self::new(QualificationPhase::Cold, RUN_ID, None, false)
    }

    fn warm_without_admission() -> Self {
        Self::new(
            QualificationPhase::Warm,
            RUN_ID + 1,
            Some(QualificationRunRef {
                run_id: RUN_ID,
                run_attempt: 1,
            }),
            true,
        )
    }

    fn new(
        phase: QualificationPhase,
        run_id: u64,
        predecessor: Option<QualificationRunRef>,
        missing_admission: bool,
    ) -> Self {
        let runner_temp = tempfile::tempdir().expect("runner temp");
        let plan = qualification_plan(phase, run_id, predecessor);
        let run_dir = runner_temp.path().join("velnor").join(&plan.run_key);
        fs::create_dir_all(&run_dir).expect("run directory");
        fs::write(
            run_dir.join("plan.json"),
            serde_json::to_vec(&plan).expect("plan JSON"),
        )
        .expect("plan artifact");
        let matrix_key = plan.matrix.include[0].matrix_key.clone();
        let entry = &plan.matrix.include[0];
        let layer = "cargo_sources";
        let (directive_json, runtime_identity_json) = if missing_admission {
            (String::new(), String::new())
        } else {
            let directive = QualificationCacheDirective::for_plan(&plan, None)
                .expect("cold directive")
                .unwrap_or_else(|| panic!("qualification directive missing"));
            let layer_type = parse_layer(layer).expect("cache layer");
            let requirements = directive
                .layer(&entry.matrix_key, layer_type)
                .expect("layer directive")
                .runtime
                .clone();
            (
                canonical_json_str(&directive).expect("directive JSON"),
                canonical_json_str(&runtime_identity(
                    requirements
                        .as_ref()
                        .expect("active cache runtime requirements"),
                ))
                .expect("runtime identity"),
            )
        };
        if !directive_json.is_empty() {
            fs::write(
                run_dir.join(QUALIFICATION_CACHE_DIRECTIVE_FILENAME),
                &directive_json,
            )
            .expect("directive artifact");
        }
        Self {
            runner_temp,
            plan,
            directive_json,
            runtime_identity_json,
            matrix_key,
            layer: layer.to_owned(),
        }
    }

    fn request(&self) -> QualificationCacheBindingRequest<'_> {
        let context = self.plan.qualification.as_ref().expect("context");
        QualificationCacheBindingRequest {
            runner_temp: self.runner_temp.path(),
            run_id: context.run_id,
            run_attempt: context.run_attempt,
            event_name: "workflow_dispatch",
            repository: &context.repository,
            git_ref: &context.git_ref,
            ref_protected: context.ref_protected,
            workflow_ref: &context.workflow_ref,
            workflow_sha: &context.workflow_sha,
            source_sha: &context.source_sha,
            runner_os: "Linux",
            runner_arch: "X64",
            image_os: Some("ubuntu26"),
            image_version: Some("2026.10.18"),
            matrix_key: &self.matrix_key,
            layer: &self.layer,
            runtime_identity_json: &self.runtime_identity_json,
        }
    }

    fn write_plan(&self, plan: &Plan) {
        let path = self
            .runner_temp
            .path()
            .join("velnor")
            .join(&plan.run_key)
            .join("plan.json");
        fs::write(path, serde_json::to_vec(plan).expect("plan JSON")).expect("rewrite plan");
    }

    fn write_directive(&self, value: &str) {
        let path = self
            .runner_temp
            .path()
            .join("velnor")
            .join(&self.plan.run_key)
            .join(QUALIFICATION_CACHE_DIRECTIVE_FILENAME);
        fs::write(path, value).expect("rewrite directive");
    }
}

#[cfg(unix)]
#[test]
fn plan_and_admission_fifos_are_rejected_without_blocking() {
    let temp = tempfile::tempdir().expect("temporary directory");
    for name in [
        "plan.json",
        crate::qualification_resolver::QUALIFICATION_ADMISSION_FILENAME,
    ] {
        let fifo = temp.path().join(name);
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo");
        assert!(status.success(), "mkfifo returned {status}");

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = read_owned_runner_file(&fifo, 128);
            sender
                .send(result.is_err())
                .expect("test is waiting for the FIFO result");
        });
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("FIFO open must not block")
        );
    }
}

fn bind(request: &QualificationCacheBindingRequest<'_>) -> QualificationCacheBindingOutputs {
    bind_qualification_cache_runtime(request).expect("cache binding")
}

fn bind_result(
    request: &QualificationCacheBindingRequest<'_>,
) -> Result<QualificationCacheBindingOutputs, OrchestratorError> {
    bind_qualification_cache_runtime(request)
}

fn qualification_plan(
    phase: QualificationPhase,
    run_id: u64,
    predecessor: Option<QualificationRunRef>,
) -> Plan {
    let package_id = "path+file:///workspace/crates/demo#demo@0.1.0".to_owned();
    let task_digest = digest_b3(b"cache runtime task");
    let input_digest = digest_b3(b"cache runtime input");
    let run_key = run_key_for_ci(run_id, 1);
    let platform =
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("planned platform");
    let entry = qualification_entry(&task_digest, &input_digest, &run_key, &platform);
    Plan {
        schema: Plan::SCHEMA,
        run_key: run_key.clone(),
        plan_id: plan_id_for_run(&run_key).expect("plan id"),
        base: None,
        head: SOURCE.to_owned(),
        event: WorkflowEvent::Qualification,
        qualification: Some(QualificationDispatch {
            mode: velnor_actions_contract::QualificationDispatchMode::Controlled,
            campaign: "cache-runtime-tests".to_owned(),
            phase,
            repository: REPOSITORY.to_owned(),
            default_branch: "main".to_owned(),
            git_ref: "refs/heads/main".to_owned(),
            ref_protected: true,
            workflow_ref: WORKFLOW_REF.to_owned(),
            workflow_sha: SOURCE.to_owned(),
            source_sha: SOURCE.to_owned(),
            run_id,
            run_attempt: 1,
            predecessor,
            scale_set_image_source_sha: None,
        }),
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(Some("qualification_bypass")).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "c".repeat(64),
        },
        packages: vec![PlanPackage {
            package_id,
            name: "demo".to_owned(),
            manifest: "crates/demo/Cargo.toml".to_owned(),
            selected: true,
            reasons: vec!["changed".to_owned()],
            tasks: vec!["stack/rust/demo/test/default".to_owned()],
        }],
        obligations: vec![PlanObligation {
            task_id: "stack/rust/demo/test/default".to_owned(),
            decision: ObligationDecision::Execute,
            reason: "qualification_full".to_owned(),
            task_digest,
            input_digest,
            closure_digest: digest_b3(b"cache runtime closure"),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec!["stack/rust/demo/test/default".to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

fn qualification_entry(
    task_digest: &str,
    input_digest: &str,
    run_key: &str,
    platform: &PlannedPlatform,
) -> MatrixEntry {
    let package_id = "path+file:///workspace/crates/demo#demo@0.1.0";
    let mut entry = MatrixEntry::derive(
        "rust",
        "stack/rust/demo/test/default",
        "mise run test",
        task_digest,
        serde_json::json!({
            "package_id": package_id,
            "unit_id": package_id,
            "compile_driver": "cargo"
        }),
        ExecuteTaskIds::default(),
        input_digest,
        run_key,
        "rust-demo",
        platform.clone(),
    )
    .expect("matrix entry");
    let toolchain_inputs = test_toolchain_inputs();
    let expected_toolchain_id = toolchain_id(&toolchain_inputs).expect("toolchain identity");
    entry.cache_ids = Some(
        EntryCacheIds::new(
            &digest_b3(b"workspace"),
            &digest_b3(b"lane"),
            platform.platform_id.as_str(),
            &expected_toolchain_id,
            &digest_b3(b"cache format"),
        )
        .expect("cache identities"),
    );
    entry.toolchain_inputs = Some(toolchain_inputs);
    entry.toolchain_id = Some(expected_toolchain_id);
    entry
}

fn runtime_identity(
    requirements: &QualificationRuntimeIdentityRequirements,
) -> QualificationRuntimeIdentity {
    let tools = requirements
        .toolchain_inputs
        .tools
        .iter()
        .map(|selector| {
            let (_, version) = selector.rsplit_once('@').expect("pinned tool selector");
            QualificationRuntimeToolObservation {
                selector: selector.clone(),
                reported_version: version.to_owned(),
                executable_sha256: "a".repeat(64),
            }
        })
        .collect();
    let mut abi = vec![QualificationRuntimeAbiObservation::Host {
        os: "Linux".to_owned(),
        arch: "x86_64".to_owned(),
        kernel: "6.12.0".to_owned(),
        libc: Some("glibc 2.39".to_owned()),
    }];
    if let Some(version) = requirements
        .toolchain_inputs
        .tools
        .iter()
        .find_map(|selector| selector.strip_prefix("rust@"))
    {
        abi.push(QualificationRuntimeAbiObservation::Rust {
            host: "x86_64-unknown-linux-gnu".to_owned(),
            target: requirements.target.clone(),
            version_verbose: format!(
                "rustc {version} (test build)\nhost: x86_64-unknown-linux-gnu\nrelease: {version}"
            ),
            target_cfg: "target_arch=\"x86_64\"\ntarget_os=\"linux\"".to_owned(),
            installed_components: requirements
                .toolchain_inputs
                .components
                .iter()
                .filter(|component| matches!(component.as_str(), "clippy" | "rustfmt"))
                .cloned()
                .collect(),
        });
    }
    QualificationRuntimeIdentity {
        platform: QualificationRuntimePlatform {
            os: "Linux".to_owned(),
            arch: "X64".to_owned(),
            runner_label: requirements.runner_label.clone(),
            runs_on: requirements.runs_on.clone(),
            image_identity: QualificationRuntimeImageIdentity::Hosted {
                image_os: "ubuntu26".to_owned(),
                image_version: "2026.10.18".to_owned(),
            },
            target: requirements.target.clone(),
        },
        tools,
        abi,
    }
}

fn test_toolchain_inputs() -> ToolchainInputs {
    ToolchainInputs {
        tools: vec!["rust@1.98.1".to_owned()],
        components: vec!["clippy".to_owned(), "rustfmt".to_owned()],
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    }
}
