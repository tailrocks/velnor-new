use super::*;
use velnor_actions_contract::StepKind;

fn field<'a>(node: &'a Yaml, key: &str) -> &'a Yaml {
    let Yaml::Map(fields) = node else {
        panic!("mapping")
    };
    &fields.iter().find(|(name, _)| name == key).expect(key).1
}

fn env_value<'a>(step: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    let Yaml::Map(fields) = step else {
        return None;
    };
    let (_, Yaml::Map(environment)) = fields.iter().find(|(name, _)| name == "env")? else {
        return None;
    };
    environment
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

fn has_operation(step: &Yaml, operation: &str) -> bool {
    env_value(step, "VELNOR_INTERNAL_OP") == Some(&Yaml::str(operation))
}

fn with_sources() -> ConsumerReleaseSpec {
    let mut spec = fixture();
    for root in ["", "nested/"] {
        let step = crate::ambient_shell_step(
            &format!("Prepare {root}Cargo.toml"),
            vec![
                "sh".to_owned(),
                "-c".to_owned(),
                format!("echo prepare-{root}Cargo.toml"),
            ],
            spec.producer_cargo_env.clone(),
        )
        .expect("source step");
        spec.producer_source_prep.push(step);
    }
    spec
}

#[test]
fn source_selection_checks_out_authenticated_a_only_for_native_finalization() {
    let spec = fixture();
    let rendered = render_preview(&spec).expect("preview").bytes;
    let eligibility = job(&rendered, "preview-eligibility", Some("preview-payload"));
    assert!(eligibility.contains("select-preview-source-v1"));
    assert!(eligibility.contains("finalize-preview-source-v1"));
    let select = eligibility
        .find("select-preview-source-v1")
        .expect("source selector");
    let checkout = eligibility
        .find("actions/checkout@")
        .expect("exact-source checkout");
    let finalize = eligibility
        .find("finalize-preview-source-v1")
        .expect("source finalizer");
    assert!(select < checkout && checkout < finalize);
    assert!(eligibility.contains("ref: ${{ steps.source-select.outputs.source_sha }}"));
    assert!(eligibility.contains("lfs: ${{ steps.source-select.outputs.checkout_lfs }}"));
    assert!(eligibility.contains("checkout_lfs: ${{ steps.source-select.outputs.checkout_lfs }}"));
    assert!(eligibility.contains(
        "VELNOR_REQUEST_FILE: ${{ runner.temp }}/velnor/preview-source-seed-policy.json"
    ));
    assert!(eligibility.contains("fetch-depth: 0"));
    assert!(eligibility.contains("persist-credentials: false"));
    assert!(eligibility[..checkout].contains("GH_TOKEN:"));
    assert!(eligibility[..checkout].contains("${{ github.token }}"));
    assert!(eligibility[..checkout].contains("VELNOR_RELEASE_STATUS_TOKEN:"));
    assert!(!eligibility[checkout..finalize].contains("GH_TOKEN:"));
    assert!(!eligibility[checkout..finalize].contains("VELNOR_RELEASE_STATUS_TOKEN:"));
    assert!(eligibility[finalize..].contains("GH_TOKEN: ${{ github.token }}"));
    assert!(eligibility[finalize..].contains("VELNOR_RELEASE_STATUS_TOKEN:"));
    assert!(eligibility[finalize..].contains("$RUNNER_TEMP/velnor/bin/velnor-actions-"));
    assert!(
        eligibility.contains(
            "VELNOR_RELEASE_CI_RUN_ATTEMPT: ${{ steps.source-select.outputs.ci_attempt }}"
        )
    );
    assert!(
        eligibility
            .contains("velnor-preview-source-r${{ github.run_id }}-a${{ github.run_attempt }}")
    );
    assert!(eligibility.contains("preview-source-admission.json"));
    assert!(eligibility.contains("id: source-admission-upload"));
    for output in [
        "source_sha",
        "workflow_authority_sha",
        "ci_run_id",
        "ci_attempt",
        "commit_count",
        "release_policy",
        "payload_matrix",
        "producer_runner",
        "producer_timeout_minutes",
        "source_admission_artifact_id",
        "source_admission_artifact_digest",
        "source_admission_sha256",
        "checkout_lfs",
    ] {
        assert!(
            eligibility.contains(&format!("{output}:")),
            "missing {output}"
        );
    }
}

#[test]
fn authenticated_source_lfs_flag_controls_every_preview_source_checkout() {
    let mut spec = super::with_tap();
    spec.checkout_lfs = false;
    let rendered = render_preview(&spec).expect("preview with tap").bytes;
    let eligibility = job(&rendered, "preview-eligibility", Some("preview-payload"));
    assert!(eligibility.contains("lfs: ${{ steps.source-select.outputs.checkout_lfs }}"));
    let payload = job(&rendered, "preview-payload", Some("preview-assemble"));
    assert!(payload.contains("lfs: ${{ needs.preview-eligibility.outputs.checkout_lfs }}"));
    let assembly = job(&rendered, "preview-assemble", Some("preview-signer"));
    assert!(assembly.contains("lfs: ${{ needs.preview-eligibility.outputs.checkout_lfs }}"));
    let tap_prepare = job(&rendered, "preview-tap-prepare", Some("preview-tap-writer"));
    assert!(tap_prepare.contains("lfs: ${{ needs.preview-eligibility.outputs.checkout_lfs }}"));
    assert!(!rendered.contains("lfs: true"));
}

#[test]
fn source_admission_acquire_owns_policy_materialization() {
    let spec = fixture();
    let jobs = production::jobs(&spec).expect("production jobs");
    let payload = &jobs[0].1;
    let Yaml::Seq(steps) = field(payload, "steps") else {
        panic!("payload steps")
    };
    let acquire = steps
        .iter()
        .find(|step| has_operation(step, "acquire-preview-source-v1"))
        .expect("source admission acquire");
    assert_eq!(
        env_value(acquire, "VELNOR_REQUEST_FILE"),
        Some(&Yaml::str(
            "${{ runner.temp }}/velnor/package-release-policy.json"
        ))
    );
    assert_eq!(
        env_value(acquire, "VELNOR_RELEASE_STATUS_TOKEN"),
        Some(&Yaml::str("${{ secrets.RELEASE_STATUS_READ_TOKEN }}"))
    );
    let Yaml::Str(run) = field(acquire, "run") else {
        panic!("acquire run")
    };
    assert!(!run.contains("printf '%s' \"$VELNOR_RELEASE_POLICY\" > \"$VELNOR_REQUEST_FILE\""));
}

#[test]
fn every_preview_operation_binds_both_sha_identities_and_admission_locators() {
    let spec = fixture();
    let rendered = render_preview(&spec).expect("preview").bytes;
    let payload = job(&rendered, "preview-payload", Some("preview-assemble"));
    let assembly = job(&rendered, "preview-assemble", Some("preview-signer"));
    for yaml in [payload, assembly] {
        for name in [
            "VELNOR_RELEASE_SOURCE_SHA",
            "VELNOR_RELEASE_WORKFLOW_AUTHORITY_SHA",
            "VELNOR_RELEASE_SOURCE_ADMISSION_ARTIFACT_ID",
            "VELNOR_RELEASE_SOURCE_ADMISSION_ARTIFACT_DIGEST",
            "VELNOR_RELEASE_SOURCE_ADMISSION_SHA256",
        ] {
            assert!(yaml.contains(&format!("{name}:")), "missing {name}");
        }
        assert!(!yaml.contains("VELNOR_RELEASE_NEEDS_JSON"));
    }
}

#[test]
fn producer_helper_bootstrap_selects_only_trusted_manifest_host_assets() {
    let spec = fixture();
    let rendered = render_preview(&spec).expect("preview").bytes;
    let producer = job(&rendered, "preview-payload", Some("preview-assemble"));
    for target in velnor_actions_contract::targets::ReleaseTarget::ALL {
        assert!(producer.contains(&format!("acquire-{}", target.triple())));
        assert!(producer.contains(&format!("Set up Velnor-owned Mise for {}", target.triple())));
    }
    for host in [
        "runner.os == 'Linux' && runner.arch == 'X64'",
        "runner.os == 'macOS' && runner.arch == 'ARM64'",
        "runner.os == 'macOS' && runner.arch == 'X64'",
    ] {
        assert!(producer.contains(host), "missing host selector {host}");
    }
    assert!(producer.contains("Reject unsupported Velnor SDK host"));
    assert!(producer.contains("VELNOR_RELEASE_COMMIT: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));
    assert!(
        !producer.contains("VELNOR_ASSET_URL: ${{ needs.preview-eligibility.outputs.source_sha }}")
    );
}

#[test]
fn preview_uses_authenticated_checkout_without_replaying_b_metadata() {
    let spec = with_sources();
    let rendered = render_preview(&spec).expect("preview sources");
    let yaml = rendered.bytes;
    let producer = job(&yaml, "preview-payload", Some("preview-assemble"));
    super::assert_production_credentials_are_native_only(&spec);
    let checkout = producer.find("actions/checkout@").expect("source checkout");
    let execute = producer
        .find("execute-release-producer-v1")
        .expect("native source task");
    assert!(checkout < execute);
    for forbidden in [
        "Create owned Cargo metadata homes",
        "Install pinned Cargo metadata toolchain",
        "prepare-Cargo.toml",
        "prepare-nested/Cargo.toml",
        "RUSTUP_TOOLCHAIN:",
        "MISE_CARGO_HOME:",
        "MISE_RUSTUP_HOME:",
    ] {
        assert!(
            !producer.contains(forbidden),
            "B metadata leaked: {forbidden}"
        );
    }
    for forbidden in [
        "id-token:",
        "contents: 'write'",
        "environment:",
        "actions/cache",
    ] {
        assert!(
            !producer.contains(forbidden),
            "producer must not gain {forbidden}"
        );
    }
    for name in ["preview-signer", "preview-publisher", "preview-recovery"] {
        let isolated = job(&yaml, name, None);
        assert!(!isolated.contains("prepare-Cargo.toml"));
        assert!(!isolated.contains("MISE_CARGO_HOME"));
    }
}

#[test]
fn preview_steps_do_not_export_b_toolchain_or_home_environment() {
    let spec = with_sources();
    let (_, Yaml::Map(fields)) = production::jobs(&spec).expect("producer").remove(0) else {
        unreachable!()
    };
    let expected_policy = "${{ needs.preview-eligibility.outputs.release_policy }}";
    let job_env = fields
        .iter()
        .find(|(key, _)| key == "env")
        .expect("public plan env");
    assert_eq!(
        job_env.1,
        Yaml::Map(vec![(
            "VELNOR_RELEASE_POLICY".to_owned(),
            Yaml::str(expected_policy),
        )])
    );
    let Some((_, Yaml::Seq(steps))) = fields.iter().find(|(key, _)| key == "steps") else {
        unreachable!()
    };
    assert!(
        !steps
            .iter()
            .any(|step| { field(step, "name") == &Yaml::str(crate::VERIFY_RUST_TOOLCHAIN_STEP) }),
        "preview production retains its early return before Rust bootstrap"
    );
    for step in steps {
        let Yaml::Map(fields) = step else {
            unreachable!()
        };
        if let Some((_, Yaml::Map(env))) = fields.iter().find(|(key, _)| key == "env") {
            for key in spec.producer_cargo_env.keys() {
                assert!(
                    !env.iter().any(|(name, _)| name == key),
                    "B env leaked: {key}"
                );
            }
        }
    }
}

#[test]
fn preview_ignores_b_source_preparation_but_rejects_credential_environment() {
    let mut spec = with_sources();
    let StepKind::Shell { env, .. } = &mut spec.producer_source_prep[0].kind else {
        unreachable!()
    };
    env.insert(
        "CARGO_HOME".to_owned(),
        "/historical-b-workspace/cargo".to_owned(),
    );
    let rendered = render_preview(&spec).expect("A owns its preparation");
    assert!(!rendered.bytes.contains("historical-b-workspace"));
    assert!(!rendered.bytes.contains("prepare-Cargo.toml"));
    let mut spec = with_sources();
    spec.producer_cargo_env
        .insert("GH_TOKEN".to_owned(), "forbidden".to_owned());
    assert!(render_preview(&spec).is_err());
}

#[test]
fn historical_b_toolchain_symbols_do_not_enter_preview_source_workflow() {
    let mut spec = with_sources();
    spec.install_rust_argv[3] = "rust@${VELNOR_TOOL_RUST}".to_owned();
    spec.producer_cargo_env.insert(
        "RUSTUP_TOOLCHAIN".to_owned(),
        "${VELNOR_TOOL_RUST}".to_owned(),
    );
    for prep in &mut spec.producer_source_prep {
        let StepKind::Shell { env, .. } = &mut prep.kind else {
            panic!("source shell")
        };
        env.insert(
            "RUSTUP_TOOLCHAIN".to_owned(),
            "${VELNOR_TOOL_RUST}".to_owned(),
        );
    }
    let actual = render_preview(&spec)
        .expect("full symbolic companion")
        .bytes;
    let mut document = Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(production::jobs(&spec).expect("composed production")),
    )]);
    crate::project_tool_resolver::bind_workflow(&mut document).expect("symbolic binding");
    let text = crate::yaml::render_yaml(&document);
    let payload = job(&text, "preview-payload", Some("preview-assemble"));
    let checkout = payload.find("actions/checkout@").expect("checkout");
    let execute = payload
        .find("execute-release-producer-v1")
        .expect("source task");
    assert!(checkout < execute);
    for forbidden in [
        "VELNOR_TOOL_RUST",
        "RUSTUP_TOOLCHAIN:",
        "Install pinned Cargo metadata toolchain",
        "prepare-Cargo.toml",
        "Resolve project tool versions",
    ] {
        assert!(
            !payload.contains(forbidden),
            "B tool metadata leaked: {forbidden}"
        );
        assert!(
            !actual.contains(forbidden),
            "B tool metadata leaked into the generated workflow: {forbidden}"
        );
    }

    let stable = super::stable::stable();
    let stable_jobs = production::jobs(&stable).expect("stable production graph");
    let stable_payload = crate::yaml::render_yaml(&Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(stable_jobs),
    )]));
    assert!(stable_payload.contains("Create owned Cargo metadata homes"));
    assert!(stable_payload.contains("Install pinned Cargo metadata toolchain"));
}
