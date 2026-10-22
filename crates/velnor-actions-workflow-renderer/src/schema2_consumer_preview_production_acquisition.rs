//! Source checkout and artifact acquisition steps for producer jobs.

use std::collections::BTreeMap;

use super::{
    ConsumerReleaseSpec, PackageReleaseChannel, RenderError, Yaml, acquire, join_argv_for_run,
    non_internal_step_to_yaml, operation_env, production_operation, run_step, set_id, setup, steps,
};

pub(super) fn acquisition_steps(
    spec: &ConsumerReleaseSpec,
    id: &str,
    needed: bool,
) -> Result<Vec<Yaml>, RenderError> {
    if spec.policy.channel == PackageReleaseChannel::Preview {
        let mut result = preview_producer_acquisition_steps(spec, id)?;
        let (mut admission, _) = production_operation(spec, "acquire-preview-source-v1", true)?;
        set_id(&mut admission, "policy-admission");
        result.push(admission);
        if needed {
            let (mut admission, _) = production_operation(spec, "acquire-release-leaves-v1", true)?;
            set_id(&mut admission, "acquire");
            operation_env(&mut admission, &[("VELNOR_RELEASE_STAGE", "assemble")])?;
            result.push(admission);
        }
        return Ok(result);
    }
    let stable = spec.policy.channel
        == velnor_actions_contract::package_release::PackageReleaseChannel::Stable;
    let mut result = vec![
        setup(&spec.producer_setup),
        run_step(
            "Install pinned GitHub CLI",
            &join_argv_for_run(&spec.install_gh_argv)?,
        ),
        acquire(&spec.producer_acquire, id, spec.producer_runner.runs_on())?,
    ];
    if stable {
        let (mut origin, _) = production_operation(spec, "admit-stable-tag-origin-v1", true)?;
        set_id(&mut origin, "origin-admission");
        result.push(origin);
    } else {
        let (mut admission, _) = production_operation(spec, "acquire-preview-source-v1", true)?;
        set_id(&mut admission, "policy-admission");
        result.push(admission);
    }
    if needed {
        let (mut admission, _) = production_operation(spec, "acquire-release-leaves-v1", true)?;
        set_id(&mut admission, "acquire");
        operation_env(
            &mut admission,
            if spec.policy.channel == PackageReleaseChannel::Preview {
                &[("VELNOR_RELEASE_STAGE", "assemble")]
            } else {
                &[
                    ("VELNOR_RELEASE_STAGE", "assemble"),
                    ("VELNOR_RELEASE_NEEDS_JSON", "${{ toJSON(needs) }}"),
                ]
            },
        )?;
        result.push(admission);
    }
    Ok(result)
}

fn preview_producer_acquisition_steps(
    spec: &ConsumerReleaseSpec,
    id: &str,
) -> Result<Vec<Yaml>, RenderError> {
    let expected = velnor_actions_contract::targets::ReleaseTarget::ALL;
    let acquires = &spec.producer_acquire_by_target;
    let setups = &spec.producer_setup_by_target;
    if acquires.len() != expected.len()
        || expected.iter().any(|target| !acquires.contains_key(target))
        || setups.len() != expected.len()
        || expected.iter().any(|target| !setups.contains_key(target))
    {
        return Err(RenderError::InvalidWorkflow(
            "preview_producer_host_pins_incomplete".to_owned(),
        ));
    }
    let mut result = vec![run_step(
        "Reject unsupported Velnor SDK host",
        r#"case "${RUNNER_OS-}/${RUNNER_ARCH-}" in Linux/X64|macOS/ARM64|macOS/X64) ;; *) echo 'unsupported native SDK host' >&2; exit 1 ;; esac"#,
    )];
    for target in expected {
        let target_setup = setups.get(&target).ok_or_else(|| {
            RenderError::InvalidWorkflow("preview_producer_setup_target_missing".to_owned())
        })?;
        target_setup.validate()?;
        let mut step = setup(target_setup);
        if let Yaml::Map(fields) = &mut step {
            if let Some((_, value)) = fields.iter_mut().find(|(key, _)| key == "name") {
                *value = Yaml::str(format!("Set up Velnor-owned Mise for {}", target.triple()));
            }
            fields.push(("if".to_owned(), Yaml::str(target_host_if(target))));
        }
        result.push(step);
    }
    result.push(run_step(
        "Install pinned GitHub CLI",
        &join_argv_for_run(&spec.install_gh_argv)?,
    ));
    for target in expected {
        let asset = acquires.get(&target).ok_or_else(|| {
            RenderError::InvalidWorkflow("preview_producer_acquire_target_missing".to_owned())
        })?;
        let runner = match target {
            velnor_actions_contract::targets::ReleaseTarget::LinuxX86_64 => "ubuntu-26.04",
            velnor_actions_contract::targets::ReleaseTarget::MacosArm64 => "macos-26",
            velnor_actions_contract::targets::ReleaseTarget::MacosX86_64 => "macos-26-intel",
        };
        let mut step = acquire(asset, id, runner)?;
        if let Yaml::Map(fields) = &mut step {
            let acquire_id = format!("acquire-{}", target.triple());
            if let Some((_, value)) = fields.iter_mut().find(|(key, _)| key == "id") {
                *value = Yaml::str(acquire_id);
            } else {
                fields.insert(1, ("id".to_owned(), Yaml::str(acquire_id)));
            }
            fields.push(("if".to_owned(), Yaml::str(target_host_if(target))));
        }
        result.push(step);
    }
    Ok(result)
}

fn target_host_if(target: velnor_actions_contract::targets::ReleaseTarget) -> &'static str {
    match target {
        velnor_actions_contract::targets::ReleaseTarget::LinuxX86_64 => {
            "${{ runner.os == 'Linux' && runner.arch == 'X64' }}"
        }
        velnor_actions_contract::targets::ReleaseTarget::MacosArm64 => {
            "${{ runner.os == 'macOS' && runner.arch == 'ARM64' }}"
        }
        velnor_actions_contract::targets::ReleaseTarget::MacosX86_64 => {
            "${{ runner.os == 'macOS' && runner.arch == 'X64' }}"
        }
    }
}
pub(super) fn source_steps(spec: &ConsumerReleaseSpec, id: &str) -> Result<Vec<Yaml>, RenderError> {
    let runner = spec.producer_runner.runs_on();
    crate::toolchain_env::check_toolchain_homes(&spec.producer_cargo_env)?;
    crate::toolchain_env::reject_denied_step_keys(&spec.producer_cargo_env)?;
    let checkout = steps::checkout(
        spec,
        super::super::TapInvocationContext::for_channel(spec.policy.channel).source(),
    );
    let mut producer_steps = vec![
        checkout,
        run_step(
            "Create owned Cargo metadata homes",
            "mkdir -p \"$MISE_CARGO_HOME\" \"$MISE_RUSTUP_HOME\"",
        ),
        run_step(
            "Install pinned Cargo metadata toolchain",
            &join_argv_for_run(&spec.install_rust_argv)?,
        ),
        run_step(
            crate::VERIFY_RUST_TOOLCHAIN_STEP,
            &join_argv_for_run(&spec.verify_rust_argv)?,
        ),
    ];
    for step in &spec.producer_source_prep {
        let velnor_actions_contract::StepKind::Shell { env, .. } = &step.kind else {
            return Err(RenderError::InvalidWorkflow(
                "preview_source_preparation_not_shell".to_owned(),
            ));
        };
        if env != &spec.producer_cargo_env {
            return Err(RenderError::InvalidWorkflow(
                "preview_source_preparation_home_mismatch".to_owned(),
            ));
        }
        producer_steps.push(non_internal_step_to_yaml(
            id,
            step,
            &BTreeMap::new(),
            runner,
        )?);
    }
    Ok(producer_steps)
}
