use super::*;

use crate::cachekey::ToolchainInputs;
use crate::ids::{plan_id_for_run, run_key_for_ci};
use crate::workflow::{
    EntryCacheIds, ExecuteTaskIds, MatrixEntry, ObligationDecision, Plan, PlanBaseline,
    PlanGenerator, PlanMatrix, PlanObligation, PlanPackage, PlanRunner, PlannedPlatform,
    QualificationDispatch, QualificationPhase, QualificationRuntimeAbiObservation,
    QualificationRuntimeIdentity, QualificationRuntimeIdentityRequirements,
    QualificationRuntimeImageDigests, QualificationRuntimeImageIdentity,
    QualificationRuntimePlatform, QualificationRuntimeScaleSet,
    QualificationRuntimeToolObservation, WorkflowEvent,
};
use crate::{RunnerSelection, Trust, digest_b3};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn serialized_disabled_layers_cannot_authorize_cache_access() -> TestResult {
    for (phase, layer) in [
        (
            QualificationPhase::Control,
            QualificationCacheLayer::CargoSources,
        ),
        (
            QualificationPhase::Cold,
            QualificationCacheLayer::TaskResult,
        ),
        (
            QualificationPhase::Cold,
            QualificationCacheLayer::MbxObjects,
        ),
    ] {
        let plan = qualification_plan(phase, 700, &"a".repeat(40), None)?;
        let entry = &plan.matrix.include[0];
        let directive = QualificationCacheDirective::for_plan(&plan, None)?
            .ok_or_else(|| std::io::Error::other("qualification directive missing"))?;
        let layer_directive = directive
            .layer(&entry.matrix_key, layer)
            .ok_or_else(|| std::io::Error::other("layer directive missing"))?;
        assert!(!layer_directive.active);
        let evidence = runtime_identity(&layer_directive.runtime);
        let mut serialized = serde_json::to_value(&directive)?;
        let layers = serialized["lanes"][0]["layers"]
            .as_array_mut()
            .ok_or_else(|| std::io::Error::other("layers are not an array"))?;
        let forged = layers
            .iter_mut()
            .find(|value| value["layer"] == serde_json::json!(layer))
            .ok_or_else(|| std::io::Error::other("target layer missing"))?;
        forged["active"] = serde_json::json!(true);
        forged["restore"] = serde_json::json!({"slot":"k1","expected_cache":null});
        forged["restore_policy"] = serde_json::json!("admission_gated");
        forged["save_policy"] = serde_json::json!("k1");

        let forged: QualificationCacheDirective = serde_json::from_value(serialized)?;
        assert!(
            forged
                .bind_runtime(&plan, None, &entry.matrix_key, layer, &evidence)
                .is_err(),
            "{phase:?} forged {layer:?} directive must fail plan reconstruction"
        );
    }
    Ok(())
}

#[test]
fn plan_reconstructed_cold_directive_binds_runtime_identity() -> TestResult {
    let plan = qualification_plan(QualificationPhase::Cold, 700, &"a".repeat(40), None)?;
    plan.validate()?;
    let entry = &plan.matrix.include[0];
    let directive = QualificationCacheDirective::for_plan(&plan, None)?
        .ok_or_else(|| std::io::Error::other("qualification directive missing"))?;
    let layer_directive = directive
        .layer(&entry.matrix_key, QualificationCacheLayer::CargoSources)
        .ok_or_else(|| std::io::Error::other("layer directive missing"))?;
    let keys = directive.bind_runtime(
        &plan,
        None,
        &entry.matrix_key,
        QualificationCacheLayer::CargoSources,
        &runtime_identity(&layer_directive.runtime),
    )?;
    assert!(keys.restore.is_some());
    assert!(keys.save_key.is_some());
    Ok(())
}

#[test]
fn qualification_identity_requires_raw_toolchain_inputs() -> TestResult {
    let mut plan = qualification_plan(QualificationPhase::Cold, 700, &"a".repeat(40), None)?;
    plan.matrix.include[0].toolchain_inputs = None;
    assert!(QualificationCacheDirective::for_plan(&plan, None).is_err());
    Ok(())
}

#[test]
fn runtime_identity_binds_actual_tools_and_abi_facts() -> TestResult {
    let plan = qualification_plan(QualificationPhase::Cold, 700, &"a".repeat(40), None)?;
    let directive = QualificationCacheDirective::for_plan(&plan, None)?
        .ok_or_else(|| std::io::Error::other("qualification directive missing"))?;
    let entry = &plan.matrix.include[0];
    let runtime = &directive
        .layer(&entry.matrix_key, QualificationCacheLayer::CargoSources)
        .ok_or_else(|| std::io::Error::other("cache layer missing"))?
        .runtime;
    let mut evidence = runtime_identity(runtime);
    let digest = validate_runtime_identity(runtime, &evidence)?;

    evidence.tools[0].reported_version = "1.98.2".to_owned();
    assert!(validate_runtime_identity(runtime, &evidence).is_err());
    evidence.tools[0].reported_version = "1.98.1".to_owned();
    evidence.tools[0].executable_sha256 = "A".repeat(64);
    assert!(validate_runtime_identity(runtime, &evidence).is_err());
    evidence.tools[0].executable_sha256 = "a".repeat(64);

    let rust_index = evidence
        .abi
        .iter()
        .position(|fact| matches!(fact, QualificationRuntimeAbiObservation::Rust { .. }))
        .ok_or_else(|| std::io::Error::other("Rust ABI fact missing"))?;
    if let QualificationRuntimeAbiObservation::Rust {
        version_verbose, ..
    } = &mut evidence.abi[rust_index]
    {
        version_verbose.push_str("\nrelease: 1.98.2");
    }
    assert!(validate_runtime_identity(runtime, &evidence).is_err());
    if let QualificationRuntimeAbiObservation::Rust {
        version_verbose, ..
    } = &mut evidence.abi[rust_index]
    {
        *version_verbose = version_verbose.replace("\nrelease: 1.98.2", "");
    }

    let host_index = evidence
        .abi
        .iter()
        .position(|fact| matches!(fact, QualificationRuntimeAbiObservation::Host { .. }))
        .ok_or_else(|| std::io::Error::other("host ABI fact missing"))?;
    if let QualificationRuntimeAbiObservation::Host { arch, .. } = &mut evidence.abi[host_index] {
        *arch = "aarch64".to_owned();
    }
    assert!(validate_runtime_identity(runtime, &evidence).is_err());
    if let QualificationRuntimeAbiObservation::Host { arch, .. } = &mut evidence.abi[host_index] {
        *arch = "x86_64".to_owned();
    }

    let changed_kernel = {
        let mut changed = evidence.clone();
        let kernel = changed
            .abi
            .iter_mut()
            .find_map(|fact| match fact {
                QualificationRuntimeAbiObservation::Host { kernel, .. } => Some(kernel),
                _ => None,
            })
            .ok_or_else(|| std::io::Error::other("host ABI fact missing"))?;
        *kernel = "6.13.0".to_owned();
        changed
    };
    assert_ne!(digest, validate_runtime_identity(runtime, &changed_kernel)?);

    let mut java_requirements = runtime.clone();
    java_requirements.toolchain_inputs = ToolchainInputs {
        tools: vec!["java@21.0.2".to_owned()],
        components: Vec::new(),
        compile_driver: "mise".to_owned(),
        test_runner: "mise".to_owned(),
    };
    java_requirements.toolchain_id =
        crate::cachekey::toolchain_id(&java_requirements.toolchain_inputs)?;
    let java_evidence = runtime_identity(&java_requirements);
    assert!(validate_runtime_identity(&java_requirements, &java_evidence).is_ok());
    let mut missing_java = java_evidence.clone();
    missing_java
        .abi
        .retain(|fact| !matches!(fact, QualificationRuntimeAbiObservation::Java { .. }));
    assert!(validate_runtime_identity(&java_requirements, &missing_java).is_err());

    Ok(())
}

#[test]
fn scale_set_image_source_is_expected_independently_of_consumer_source() -> TestResult {
    let consumer_source = "a".repeat(40);
    let image_source = "b".repeat(40);
    let mut plan = qualification_plan(QualificationPhase::Cold, 700, &consumer_source, None)?;
    let selector = crate::config::ScaleSetSelector::try_new(
        "ubuntu-26.04-scale-set",
        &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
    )?;
    let planned = PlannedPlatform::new(&selector.token(), "x86_64-unknown-linux-gnu")?;
    let entry = &mut plan.matrix.include[0];
    entry.planned_platform = planned.clone();
    let toolchain_id = crate::cachekey::toolchain_id(
        entry
            .toolchain_inputs
            .as_ref()
            .ok_or_else(|| std::io::Error::other("toolchain inputs missing"))?,
    )?;
    entry.cache_ids = Some(EntryCacheIds::new(
        &digest_b3(b"workspace"),
        &digest_b3(b"lane"),
        &planned.platform_id,
        &toolchain_id,
        &digest_b3(b"cache format"),
    )?);
    let matrix_key = entry.matrix_key.clone();
    plan.qualification
        .as_mut()
        .ok_or_else(|| std::io::Error::other("qualification context missing"))?
        .scale_set_image_source_sha = Some(image_source.clone());
    plan.validate()?;

    let directive = QualificationCacheDirective::for_plan(&plan, None)?
        .ok_or_else(|| std::io::Error::other("qualification directive missing"))?;
    let layer = QualificationCacheLayer::CargoSources;
    let runtime = directive
        .layer(&matrix_key, layer)
        .ok_or_else(|| std::io::Error::other("cache layer missing"))?
        .runtime
        .clone();
    assert_eq!(
        runtime.scale_set_image_source_sha.as_deref(),
        Some(image_source.as_str())
    );
    assert_ne!(consumer_source, image_source);

    let identity = scale_set_runtime_identity(&runtime, &selector, &image_source);
    assert!(
        directive
            .bind_runtime(&plan, None, &matrix_key, layer, &identity)
            .is_ok()
    );

    let wrong_image_source = "c".repeat(40);
    let wrong_identity = scale_set_runtime_identity(&runtime, &selector, &wrong_image_source);
    assert!(
        directive
            .bind_runtime(&plan, None, &matrix_key, layer, &wrong_identity)
            .is_err()
    );

    plan.qualification
        .as_mut()
        .ok_or_else(|| std::io::Error::other("qualification context missing"))?
        .scale_set_image_source_sha = None;
    let missing_source_directive = QualificationCacheDirective::for_plan(&plan, None)?
        .ok_or_else(|| std::io::Error::other("qualification directive missing"))?;
    let missing_runtime = missing_source_directive
        .layer(&matrix_key, layer)
        .ok_or_else(|| std::io::Error::other("cache layer missing"))?
        .runtime
        .clone();
    assert!(
        missing_source_directive
            .bind_runtime(
                &plan,
                None,
                &matrix_key,
                layer,
                &scale_set_runtime_identity(&missing_runtime, &selector, &image_source),
            )
            .is_err()
    );
    Ok(())
}

fn scale_set_runtime_identity(
    requirements: &QualificationRuntimeIdentityRequirements,
    selector: &crate::config::ScaleSetSelector,
    source_sha: &str,
) -> QualificationRuntimeIdentity {
    let image = || QualificationRuntimeImageDigests {
        archive_sha256: "1".repeat(64),
        archive_index_digest: "2".repeat(64),
        image_index_digest: Some(format!("sha256:{}", "5".repeat(64))),
        manifest_digest: format!("sha256:{}", "3".repeat(64)),
        config_digest: format!("sha256:{}", "4".repeat(64)),
    };
    let mut identity = runtime_identity(requirements);
    identity.platform.runs_on = selector.token();
    identity.platform.image_identity = QualificationRuntimeImageIdentity::ScaleSet {
        source_sha: source_sha.to_owned(),
        guest_platform: "linux/amd64".to_owned(),
        scale_set: QualificationRuntimeScaleSet {
            name: selector.name().to_owned(),
            labels: selector.labels().to_vec(),
        },
        runner_image: image(),
        dind_image: image(),
    };
    identity
}

pub(super) fn qualification_plan(
    phase: QualificationPhase,
    run_id: u64,
    source_sha: &str,
    predecessor: Option<crate::workflow::QualificationRunRef>,
) -> Result<Plan, crate::ContractError> {
    let package_id = "path+file:///workspace/crates/demo#demo@0.1.0".to_owned();
    let task_digest = digest_b3(b"test task");
    let input_digest = digest_b3(b"test input");
    let run_key = run_key_for_ci(run_id, 1);
    let entry = qualification_entry(&package_id, &task_digest, &input_digest, &run_key)?;
    let context = qualification_context(phase, source_sha, run_id, predecessor);
    Ok(Plan {
        schema: Plan::SCHEMA,
        run_key: run_key.clone(),
        plan_id: plan_id_for_run(&run_key)?,
        base: None,
        head: source_sha.to_owned(),
        event: WorkflowEvent::Qualification,
        qualification: Some(context),
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(Some("qualification_bypass"))?,
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
            input_digest,
            closure_digest: digest_b3(b"closure"),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec!["stack/rust/demo/test/default".to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),
    })
}

fn qualification_entry(
    package_id: &str,
    task_digest: &str,
    input_digest: &str,
    run_key: &str,
) -> Result<MatrixEntry, crate::ContractError> {
    let planned_platform = PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu")?;
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
        planned_platform.clone(),
    )?;
    let toolchain_inputs = test_toolchain_inputs();
    entry.cache_ids = Some(EntryCacheIds::new(
        &digest_b3(b"workspace"),
        &digest_b3(b"lane"),
        planned_platform.platform_id.as_str(),
        &crate::cachekey::toolchain_id(&toolchain_inputs)?,
        &digest_b3(b"cache format"),
    )?);
    entry.toolchain_inputs = Some(toolchain_inputs);
    Ok(entry)
}

fn qualification_context(
    phase: QualificationPhase,
    source_sha: &str,
    run_id: u64,
    predecessor: Option<crate::workflow::QualificationRunRef>,
) -> QualificationDispatch {
    QualificationDispatch {
        campaign: "directive-boundary".to_owned(),
        phase,
        repository: "tailrocks/velnor-new".to_owned(),
        default_branch: "main".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        ref_protected: true,
        workflow_ref: "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        workflow_sha: source_sha.to_owned(),
        source_sha: source_sha.to_owned(),
        run_id,
        run_attempt: 1,
        predecessor,
        scale_set_image_source_sha: None,
    }
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
    for selector in &requirements.toolchain_inputs.tools {
        let Some((name, version)) = selector.rsplit_once('@') else {
            continue;
        };
        if name == "java" || name.ends_with("/java") {
            abi.push(QualificationRuntimeAbiObservation::Java {
                version_output: format!("openjdk version \"{version}\""),
                arch: "amd64".to_owned(),
            });
        } else if matches!(name, "node" | "nodejs" | "bun") {
            abi.push(QualificationRuntimeAbiObservation::Node {
                version_output: format!("v{version}"),
                module_abi: "127".to_owned(),
                napi: "10".to_owned(),
                platform: "linux".to_owned(),
                arch: "x64".to_owned(),
            });
        } else if name == "opentofu" {
            abi.push(QualificationRuntimeAbiObservation::OpenTofu {
                version_output: format!("OpenTofu v{version}"),
                platform: "linux_amd64".to_owned(),
            });
        }
    }
    QualificationRuntimeIdentity {
        platform: QualificationRuntimePlatform {
            os: "Linux".to_owned(),
            arch: "X64".to_owned(),
            runner_label: requirements.runner_label.clone(),
            runs_on: requirements.runs_on.clone(),
            image_identity: QualificationRuntimeImageIdentity::Hosted {
                image_os: "ubuntu".to_owned(),
                image_version: "26.04".to_owned(),
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
