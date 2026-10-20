use super::*;

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

#[cfg(unix)]
#[test]
fn runtime_identity_must_be_private_and_at_the_run_owned_path() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::cold();
    let mut request = fixture.request();
    let outside_path = fixture.runner_temp.path().join("runtime-identity.json");
    request.runtime_identity_path = &outside_path;
    assert!(bind_result(&request).is_err());

    let path = fixture.runtime_identity_path.clone();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
        .expect("The test could not set the runtime identity file permissions.");
    assert!(bind_result(&fixture.request()).is_err());
}

#[cfg(unix)]
#[test]
fn runtime_identity_symlinks_are_rejected_without_reading_the_target() {
    let fixture = Fixture::cold();
    let path = fixture.runtime_identity_path.clone();
    let target = fixture
        .runner_temp
        .path()
        .join("runtime-identity-target.json");
    fs::write(
        &target,
        fs::read(&path).expect("The test could not read the original runtime identity file."),
    )
    .expect("The test could not write the runtime identity target file.");
    fs::remove_file(&path).expect("The test could not remove the original runtime identity file.");
    std::os::unix::fs::symlink(target, &path)
        .expect("The test could not create the runtime identity symbolic link.");

    assert!(bind_result(&fixture.request()).is_err());
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
