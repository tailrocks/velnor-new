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
        .expect("make runtime identity too broad");
    assert!(bind_result(&fixture.request()).is_err());
}

#[test]
fn observer_rejects_a_tool_home_outside_the_run_owned_root_before_probing() {
    let fixture = Fixture::cold();
    let mut request = fixture.request();
    request.runner_os = match std::env::consts::OS {
        "linux" => "Linux",
        "macos" => "macOS",
        "windows" => "Windows",
        _ => panic!("supported host required"),
    };
    request.runner_arch = match std::env::consts::ARCH {
        "x86_64" => "X64",
        "aarch64" => "ARM64",
        "x86" => "X86",
        _ => panic!("supported host architecture required"),
    };
    let mut reads = Vec::new();
    let error = observe_qualification_cache_runtime(&request, |name| {
        reads.push(name.to_owned());
        match name {
            "MISE_DATA_DIR" => Some(
                fixture
                    .runner_temp
                    .path()
                    .join("unowned-mise")
                    .display()
                    .to_string(),
            ),
            "RUSTUP_HOME" | "MISE_RUSTUP_HOME" => Some(
                fixture
                    .runner_temp
                    .path()
                    .join("velnor/rustup")
                    .display()
                    .to_string(),
            ),
            _ => None,
        }
    })
    .expect_err("a tool home outside the job-owned root must fail");

    assert!(
        error
            .to_string()
            .contains("qualification_runtime_tool_home_mismatch"),
        "the home check must reject before a runtime probe: {error}"
    );
    assert_eq!(reads, ["MISE_DATA_DIR"]);
}

#[test]
fn inactive_task_result_requires_the_canonical_absent_identity_path() {
    let fixture = Fixture::cold();
    let base = fixture.request();
    let filename = qualification_cache_runtime_identity_filename(&fixture.owner_id, "task_result")
        .expect("task result identity filename");
    let path = fixture
        .runner_temp
        .path()
        .join("velnor")
        .join(&fixture.plan.run_key)
        .join(filename);
    assert!(!path.exists());
    let request = QualificationCacheBindingRequest {
        layer: "task_result",
        runtime_identity_path: &path,
        ..base
    };
    let mut probes = Vec::new();
    let observed_path = observe_qualification_cache_runtime(&request, |name| {
        probes.push(name.to_owned());
        Some("unexpected-probe-value".to_owned())
    })
    .expect("inactive layer returns its canonical identity path without probing");
    assert_eq!(observed_path, path.display().to_string());
    assert!(
        probes.is_empty(),
        "inactive layer must not probe runtime facts"
    );
    assert!(
        !path.exists(),
        "inactive layer must not persist an identity file"
    );
    let outputs = bind(&request);
    assert!(!outputs.active);
    assert!(outputs.restore_key.is_none());
    assert!(outputs.save_key.is_none());

    std::fs::write(&path, b"{}").expect("unexpected inactive identity");
    assert!(bind_result(&request).is_err());
}

#[cfg(unix)]
#[test]
fn active_post_import_binding_requires_the_same_runtime_identity() {
    let fixture = Fixture::cold();
    let base = fixture.request();
    let after_name =
        qualification_cache_runtime_after_import_filename(&fixture.owner_id, &fixture.layer)
            .expect("post-import identity filename");
    let after_path = fixture
        .runner_temp
        .path()
        .join("velnor")
        .join(&fixture.plan.run_key)
        .join(after_name);
    std::fs::copy(&fixture.runtime_identity_path, &after_path)
        .expect("copy matching post-import identity");
    std::fs::set_permissions(&after_path, fs::Permissions::from_mode(0o600))
        .expect("post-import identity permissions");
    let request = QualificationCacheBindingRequest {
        runtime_identity_path: &after_path,
        ..base
    };
    assert!(bind_result(&request).is_ok());

    let directive: QualificationCacheDirective =
        serde_json::from_str(&fixture.directive_json).expect("directive");
    let requirements = directive
        .owner_layer(
            &fixture.owner_id,
            parse_layer(&fixture.layer).expect("layer"),
        )
        .and_then(|layer| layer.runtime.as_ref())
        .expect("runtime requirements");
    let mut changed = runtime_identity(requirements);
    let host = changed
        .abi
        .iter_mut()
        .find_map(|fact| match fact {
            QualificationRuntimeAbiObservation::Host { kernel, .. } => Some(kernel),
            _ => None,
        })
        .expect("host ABI observation");
    *host = "6.13.0".to_owned();
    fs::write(
        &after_path,
        canonical_json_str(&changed).expect("changed identity JSON"),
    )
    .expect("changed post-import identity");
    fs::set_permissions(&after_path, fs::Permissions::from_mode(0o600))
        .expect("changed identity permissions");
    assert!(bind_result(&request).is_err());
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
    fs::write(&target, fs::read(&path).expect("identity source")).expect("write identity target");
    fs::remove_file(&path).expect("remove identity source");
    std::os::unix::fs::symlink(target, &path).expect("identity symlink");

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

#[path = "qualification_cache_runtime_cases_plan_tests.rs"]
mod plan;
