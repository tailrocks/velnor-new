//! Shared preview producer source and host-bootstrap step construction.

use super::super::{BTreeMap, TapInvocationContext, non_internal_step_to_yaml, steps};
use super::ReleasePayloadPair;
use super::{
    ConsumerReleaseSpec, PackageReleaseChannel, RenderError, Yaml, acquire, join_argv_for_run,
    operation_env, run_step, setup, target_host_if,
};

pub(super) fn preview_producer_acquisition_steps(
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

pub(super) fn source_steps(spec: &ConsumerReleaseSpec, id: &str) -> Result<Vec<Yaml>, RenderError> {
    let checkout = steps::checkout(
        spec,
        TapInvocationContext::for_channel(spec.policy.channel).source(),
    );
    if spec.policy.channel == PackageReleaseChannel::Preview {
        crate::toolchain_env::reject_denied_step_keys(&spec.producer_cargo_env)?;
        return Ok(vec![steps::checkout_with_lfs(
            spec,
            TapInvocationContext::for_channel(spec.policy.channel).source(),
            "${{ needs.preview-eligibility.outputs.checkout_lfs }}",
        )]);
    }
    let runner = spec.producer_runner.runs_on();
    crate::toolchain_env::check_toolchain_homes(&spec.producer_cargo_env)?;
    crate::toolchain_env::reject_denied_step_keys(&spec.producer_cargo_env)?;
    let verify_step = crate::verifier_composite::verifier_shell_step(
        crate::VERIFY_RUST_TOOLCHAIN_STEP,
        spec.verify_rust_argv.clone(),
        BTreeMap::new(),
    )?;
    let verify_yaml = crate::document_steps::verifier_shell_step_to_yaml(&verify_step)?;
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
        verify_yaml,
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

pub(super) fn bind_stage(
    operation: &mut Yaml,
    spec: &ConsumerReleaseSpec,
    pair: Option<&ReleasePayloadPair>,
    preview_matrix: bool,
) -> Result<(), RenderError> {
    let env = if preview_matrix {
        vec![
            ("VELNOR_RELEASE_STAGE", "payload"),
            ("VELNOR_RELEASE_PRODUCT_ID", "${{ matrix.product_id }}"),
            ("VELNOR_RELEASE_TARGET", "${{ matrix.target }}"),
        ]
    } else if let Some(pair) = pair {
        vec![
            ("VELNOR_RELEASE_STAGE", "payload"),
            ("VELNOR_RELEASE_PRODUCT_ID", pair.product_id.as_str()),
            ("VELNOR_RELEASE_TARGET", pair.target.as_str()),
        ]
    } else if spec.policy.channel == PackageReleaseChannel::Preview {
        vec![("VELNOR_RELEASE_STAGE", "assemble")]
    } else {
        vec![
            ("VELNOR_RELEASE_STAGE", "assemble"),
            ("VELNOR_RELEASE_NEEDS_JSON", "${{ toJSON(needs) }}"),
        ]
    };
    operation_env(operation, &env)?;
    if spec.policy.channel
        == velnor_actions_contract::package_release::PackageReleaseChannel::Stable
    {
        operation_env(
            operation,
            &[(
                "VELNOR_STABLE_TAG_ORIGIN_ADMISSION_DIGEST",
                "${{ steps.origin-admission.outputs.admission_digest }}",
            )],
        )?;
    }
    if spec.policy.channel
        == velnor_actions_contract::package_release::PackageReleaseChannel::Preview
    {
        operation_env(
            operation,
            &[(
                "VELNOR_RELEASE_POLICY_ADMISSION_DIGEST",
                "${{ steps.policy-admission.outputs.admission_digest }}",
            )],
        )?;
    }
    if pair.is_none() && !preview_matrix {
        operation_env(
            operation,
            &[(
                "VELNOR_RELEASE_ADMISSION_DIGEST",
                "${{ steps.acquire.outputs.admission_digest }}",
            )],
        )?;
    }
    Ok(())
}
