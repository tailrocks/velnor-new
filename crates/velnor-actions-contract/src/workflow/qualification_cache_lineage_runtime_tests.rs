use super::*;

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
        scale_set: Box::new(QualificationRuntimeScaleSet {
            name: selector.name().to_owned(),
            labels: selector.labels().to_vec(),
        }),
        runner_image: Box::new(image()),
        dind_image: Box::new(image()),
    };
    identity
}
