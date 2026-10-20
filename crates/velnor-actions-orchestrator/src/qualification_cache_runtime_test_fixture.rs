//! Shared plan and runner fixtures for cache runtime tests.

use super::*;

pub(super) struct Fixture {
    pub(super) runner_temp: tempfile::TempDir,
    pub(super) plan: Plan,
    pub(super) directive_json: String,
    pub(super) runtime_identity_path: std::path::PathBuf,
    pub(super) owner_id: String,
    pub(super) layer: String,
}

impl Fixture {
    pub(super) fn cold() -> Self {
        Self::new(QualificationPhase::Cold, RUN_ID, None, false)
    }

    pub(super) fn warm_without_admission() -> Self {
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

    pub(super) fn new(
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
        let entry = &plan.matrix.include[0];
        let owner_id = entry.job_id.clone();
        let layer = "cargo_sources";
        let (directive_json, runtime_identity_json) = if missing_admission {
            (String::new(), String::new())
        } else {
            let directive = QualificationCacheDirective::for_plan(&plan, None)
                .expect("cold directive")
                .unwrap_or_else(|| panic!("qualification directive missing"));
            let layer_type = parse_layer(layer).expect("cache layer");
            let requirements = directive
                .owner_layer(&owner_id, layer_type)
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
        let runtime_identity_path = run_dir.join(
            qualification_cache_runtime_identity_filename(&owner_id, layer)
                .expect("identity filename"),
        );
        if !runtime_identity_json.is_empty() {
            fs::write(&runtime_identity_path, runtime_identity_json)
                .expect("runtime identity artifact");
            fs::set_permissions(&runtime_identity_path, fs::Permissions::from_mode(0o600))
                .expect("runtime identity permissions");
        }
        Self {
            runner_temp,
            plan,
            directive_json,
            runtime_identity_path,
            owner_id,
            layer: layer.to_owned(),
        }
    }

    pub(super) fn request(&self) -> QualificationCacheBindingRequest<'_> {
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
            owner_id: &self.owner_id,
            layer: &self.layer,
            runtime_identity_path: &self.runtime_identity_path,
        }
    }

    pub(super) fn write_plan(&self, plan: &Plan) {
        let path = self
            .runner_temp
            .path()
            .join("velnor")
            .join(&plan.run_key)
            .join("plan.json");
        fs::write(path, serde_json::to_vec(plan).expect("plan JSON")).expect("rewrite plan");
    }

    pub(super) fn write_directive(&self, value: &str) {
        let path = self
            .runner_temp
            .path()
            .join("velnor")
            .join(&self.plan.run_key)
            .join(QUALIFICATION_CACHE_DIRECTIVE_FILENAME);
        fs::write(path, value).expect("rewrite directive");
    }
}

pub(super) fn qualification_plan(
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
            task_digest: task_digest.clone(),
            task_binding_digest: velnor_actions_contract::TaskBindingDigest::new(task_digest)
                .expect("binding digest"),
            input_digest,
            closure_digest: digest_b3(b"cache runtime closure"),
            graph_digest: "b3-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_owned(),
            mbx_digest: "b3-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_owned(),
            profile: "default".to_owned(),
            job_id: "rust-demo".to_owned(),
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec!["stack/rust/demo/test/default".to_owned()],
        jobs: vec![velnor_actions_contract::PlanJob {
            job_id: "rust-demo".to_owned(),
            disposition: velnor_actions_contract::JobDisposition::Run,
        }],
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

pub(super) fn qualification_entry(
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

pub(super) fn runtime_identity(
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
            },
            target: requirements.target.clone(),
        },
        tools,
        abi,
    }
}

pub(super) fn test_toolchain_inputs() -> ToolchainInputs {
    ToolchainInputs {
        tools: vec!["rust@1.98.1".to_owned()],
        components: vec!["clippy".to_owned(), "rustfmt".to_owned()],
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    }
}
