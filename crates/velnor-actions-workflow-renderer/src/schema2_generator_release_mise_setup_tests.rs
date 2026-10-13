use crate::schema2::Schema2WorkflowRequest;
use crate::setup::MiseSetup;
use crate::yaml::Yaml;
use std::collections::BTreeSet;
use std::error::Error;
use velnor_actions_contract::ReleaseTarget;

#[test]
fn each_archive_consumer_has_target_pinned_mise_before_extraction() -> Result<(), Box<dyn Error>> {
    let mut pins = super::test_pins();
    pins.linux_x86_64_setup = mise_setup('a', "2026.10.4", 'b');
    pins.macos_arm64_setup = mise_setup('c', "2026.9.19", 'd');
    pins.macos_x86_64_setup = mise_setup('e', "2026.9.20", 'f');
    let request = Schema2WorkflowRequest {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::new(),
        mbx_qualification: None,
        mise_pin_qualification: None,
        rust_toolchain_qualification: None,
        product_release: Some(pins.clone()),
    };
    let release = super::super::super::generator_release(&request)?;
    let jobs = super::map_entries(super::map_field(
        super::map_entries(&release.workflow)?,
        "jobs",
    )?)?;
    assert_candidate_manifest_setup(jobs, pins.setup_for(ReleaseTarget::LinuxX86_64))?;

    for (job_id, action, target) in [
        (
            "qualify-linux",
            "./.github/actions/generator-release-qualify-linux",
            ReleaseTarget::LinuxX86_64,
        ),
        (
            "qualify-macos",
            "./.github/actions/generator-release-qualify-macos",
            ReleaseTarget::MacosArm64,
        ),
        (
            "qualify-macos-intel",
            "./.github/actions/generator-release-qualify-macos-intel",
            ReleaseTarget::MacosX86_64,
        ),
    ] {
        assert_qualifier_setup(
            &release,
            jobs,
            job_id,
            action,
            pins.setup_for(target),
            &pins,
        )?;
    }
    Ok(())
}

fn assert_candidate_manifest_setup(
    jobs: &[(String, Yaml)],
    setup: &MiseSetup,
) -> Result<(), Box<dyn Error>> {
    let candidate = super::map_field(jobs, "candidate-manifest")?;
    let steps = super::sequence(super::map_field(super::map_entries(candidate)?, "steps")?)?;
    assert!(steps.len() > 2);
    assert_setup_step(&steps[1], setup, "candidate-manifest")?;
    assert_guard_runs_after_step(steps, 1, "candidate-manifest");
    Ok(())
}

fn assert_qualifier_setup(
    release: &super::super::super::GeneratorRelease,
    jobs: &[(String, Yaml)],
    job_id: &str,
    action: &str,
    setup: &MiseSetup,
    pins: &crate::schema2::ProductReleasePins,
) -> Result<(), Box<dyn Error>> {
    let job = super::map_field(jobs, job_id)?;
    let steps = super::sequence(super::map_field(super::map_entries(job)?, "steps")?)?;
    assert_eq!(steps.len(), 2, "{job_id} must fetch source, then qualify");
    super::assert_source_step(&steps[0], job_id)?;
    super::assert_qualifier_step(&steps[1], steps, action, job_id)?;
    assert_action_extracts_guard(&release.actions, action, setup, job_id, pins)?;
    assert_eq!(
        super::map_entries(super::map_field(super::map_entries(job)?, "permissions",)?)?.len(),
        0,
        "{job_id} must retain empty job permissions"
    );
    Ok(())
}

fn assert_setup_step(step: &Yaml, setup: &MiseSetup, job_id: &str) -> Result<(), Box<dyn Error>> {
    let fields = super::map_entries(step)?;
    assert_eq!(
        super::scalar(super::map_field(fields, "name")?)?,
        "Setup Mise",
        "{job_id}"
    );
    assert_eq!(
        super::scalar(super::map_field(fields, "uses")?)?,
        setup.uses,
        "{job_id}"
    );
    let inputs = super::map_entries(super::map_field(fields, "with")?)?;
    assert_eq!(
        inputs.len(),
        6,
        "only the six qualified setup inputs are set"
    );
    assert!(
        inputs
            .iter()
            .all(|(field, _)| field != "persist_github_token"),
        "the pinned action defaults token persistence to false"
    );
    for (field, expected) in [
        ("version", setup.version.as_str()),
        ("sha256", setup.sha256.as_str()),
        ("install", "false"),
        ("env", "false"),
        ("cache", "false"),
        ("cache_save", "false"),
    ] {
        assert_eq!(
            super::scalar(super::map_field(inputs, field)?)?,
            expected,
            "{job_id} Setup Mise input {field}"
        );
    }
    Ok(())
}

fn assert_guard_runs_after_step(steps: &[Yaml], setup_index: usize, job_id: &str) {
    let guard_indices = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| {
            let fields = super::map_entries(step).ok()?;
            let run = super::map_field(fields, "run").ok()?;
            super::scalar(run)
                .ok()?
                .contains("with-owned-archive-guard.sh")
                .then_some(index)
        })
        .collect::<Vec<_>>();
    assert!(
        !guard_indices.is_empty(),
        "{job_id} has no guarded archive consumer"
    );
    assert!(
        guard_indices.iter().all(|index| *index > setup_index),
        "{job_id} must provision pinned Mise before the archive guard"
    );
}

fn assert_action_extracts_guard(
    actions: &[(String, Yaml)],
    action: &str,
    setup: &MiseSetup,
    job_id: &str,
    pins: &crate::schema2::ProductReleasePins,
) -> Result<(), Box<dyn Error>> {
    let path = format!("{}/action.yml", action.trim_start_matches("./"));
    let (_, document) = actions
        .iter()
        .find(|(candidate, _)| candidate == &path)
        .ok_or_else(|| format!("missing generated action {path}"))?;
    let runs = super::map_entries(super::map_field(super::map_entries(document)?, "runs")?)?;
    let steps = super::sequence(super::map_field(runs, "steps")?)?;
    for step in steps {
        let fields = super::map_entries(step)?;
        if fields.iter().any(|(key, _)| key == "run") {
            assert_eq!(
                super::scalar(super::map_field(fields, "shell")?)?,
                "bash",
                "{job_id} composite run steps must declare their shell"
            );
        }
    }
    assert_setup_step(
        steps.first().ok_or("qualifier action has no steps")?,
        setup,
        job_id,
    )?;
    assert_qualifier_mbx_preflight(steps, pins, job_id)?;
    assert_guard_runs_after_step(steps, 0, job_id);
    assert!(
        steps.iter().any(|step| {
            super::map_entries(step)
                .ok()
                .and_then(|fields| super::map_field(fields, "run").ok())
                .and_then(|run| super::scalar(run).ok())
                .is_some_and(|run| run.contains("with-owned-archive-guard.sh"))
        }),
        "{job_id} action must use the owned archive guard"
    );
    Ok(())
}

fn assert_qualifier_mbx_preflight(
    steps: &[Yaml],
    pins: &crate::schema2::ProductReleasePins,
    job_id: &str,
) -> Result<(), Box<dyn Error>> {
    let install_index = step_index_named(steps, "Install pinned qualification tools")?;
    let install_run = super::scalar(super::map_field(
        super::map_entries(&steps[install_index])?,
        "run",
    )?)?;
    assert!(
        !install_run.contains("mr-boxington@"),
        "{job_id} must use one action-owned MBX installation source"
    );

    let expected = pins
        .qualify_mbx_action_steps
        .iter()
        .map(crate::steps_plain::composite_step_to_yaml)
        .collect::<Result<Vec<_>, _>>()?;
    let mbx_steps = steps
        .get(install_index + 1..install_index + 1 + expected.len())
        .ok_or_else(|| format!("{job_id} is missing the action-owned MBX steps"))?;
    assert_eq!(
        mbx_steps, expected,
        "{job_id} MBX steps must match typed pins"
    );

    let native_fields = super::map_entries(&mbx_steps[1])?;
    let native_ref = super::scalar(super::map_field(native_fields, "uses")?)?;
    assert_eq!(
        native_ref,
        format!("jdx/mr-boxington-action@{}", "a".repeat(40))
    );
    let native_inputs = super::map_entries(super::map_field(native_fields, "with")?)?;
    assert_eq!(
        super::scalar(super::map_field(native_inputs, "version")?)?,
        pins.mr_boxington_version
    );
    assert_eq!(
        super::scalar(super::map_field(native_inputs, "toolchain")?)?,
        pins.rust_version
    );
    assert_eq!(pins.mr_boxington_version, "1.23.0");
    assert_eq!(pins.rust_version, "1.99.0");

    let version_check =
        super::scalar(super::map_field(super::map_entries(&mbx_steps[2])?, "run")?)?;
    assert!(version_check.contains("mbx_bin=$(command -v mbx)"));
    assert!(version_check.contains("case \"$mbx_bin\" in /*)"));
    assert!(version_check.contains("[ ! -f \"$mbx_bin\" ] || [ ! -x \"$mbx_bin\" ]"));
    assert!(version_check.contains(&format!(
        "mise --no-config --no-env --no-hooks exec rust@{} -- \"$mbx_bin\" --version",
        pins.rust_version
    )));
    assert!(!version_check.contains("-- mbx --version"));
    assert!(version_check.contains(&format!("mbx {}", pins.mr_boxington_version)));
    let qualify_index = step_index_named(steps, "Qualify downloaded candidate")?;
    assert!(
        qualify_index > install_index + expected.len(),
        "{job_id} must verify native MBX before candidate execution"
    );
    Ok(())
}

fn step_index_named(steps: &[Yaml], name: &str) -> Result<usize, Box<dyn Error>> {
    steps
        .iter()
        .position(|step| {
            let Ok(fields) = super::map_entries(step) else {
                return false;
            };
            let Ok(value) = super::map_field(fields, "name") else {
                return false;
            };
            value == &Yaml::str(name)
        })
        .ok_or_else(|| format!("missing rendered step {name}").into())
}

fn mise_setup(action: char, version: &str, digest: char) -> MiseSetup {
    MiseSetup {
        uses: format!("jdx/mise-action@{}", action.to_string().repeat(40)),
        version: version.to_owned(),
        sha256: digest.to_string().repeat(64),
    }
}
