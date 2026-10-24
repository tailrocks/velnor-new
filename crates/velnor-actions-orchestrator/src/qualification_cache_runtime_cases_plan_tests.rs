use super::*;

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
    let directive = QualificationCacheDirective::for_plan(&plan, None)
        .expect("directive for Scale Set plan")
        .expect("qualification directive");
    fixture.write_directive(&canonical_json_str(&directive).expect("directive JSON"));

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
    request.owner_id = "rust-unknown";
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
