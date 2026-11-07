// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    ReleasePlzConfig, ReleaseRenderContext, ReleaseRole, ReleaseWorkflowSpec, RenderError,
};

pub(super) fn validate_release_intent(
    spec: &ReleaseWorkflowSpec,
    ctx: &ReleaseRenderContext,
    config: &ReleasePlzConfig,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), RenderError> {
    intent.validate().map_err(RenderError::Contract)?;
    if intent.oidc_qualification != spec.qualification.is_some()
        || intent.oidc_qualification
            != (spec.plan.qualification_dispatch
                == crate::release_spec::QualificationDispatch::Enabled)
    {
        return Err(RenderError::InvalidWorkflow(
            "oidc_qualification_intent_mismatch".to_owned(),
        ));
    }
    if let Some(qualification) = &spec.qualification {
        qualification.validate_intent(intent)?;
    }
    let intent_packages: std::collections::BTreeSet<_> = intent.packages.iter().collect();
    let plan_packages: std::collections::BTreeSet<_> = spec.plan.packages.keys().collect();
    let config_packages: std::collections::BTreeSet<_> = config
        .packages
        .iter()
        .map(|package| &package.name)
        .collect();
    let publisher_role = match intent.authentication {
        velnor_actions_contract::config::ReleaseAuthentication::TrustedPublishing => {
            ReleaseRole::PublishOidc
        }
        velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken => {
            ReleaseRole::PublishBootstrap
        }
    };
    if intent.new_crates != spec.new_crates
        || intent.journal_predecessor != spec.journal_predecessor
        || !release_pin_matches(spec, intent)
        || !spec.jobs.values().any(|job| job.role == publisher_role)
        || intent.generator_version != ctx.generator_version
        || intent.release_plz_version != spec.release_plz_version
        || intent.runner != ctx.runs_on
        || velnor_actions_mise::catalog::qualified_release_plz(
            &intent.release_plz_version,
            &intent.runner,
        )
        .is_none()
        || intent.repository != spec.repository
        || spec.default_branch != intent.default_branch
        || intent.environment != spec.publish_environment
        || intent.environment != spec.bootstrap_environment
        || intent.registry != spec.plan.registry
        || intent_packages != plan_packages
        || intent_packages != config_packages
        || !release_features_match(config, intent)
        || intent.tag_template != config.tag_pattern
        || !crate::release_gates::release_intent_mode_matches(spec, intent)
    {
        return Err(RenderError::InvalidWorkflow(
            "release_intent_mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn release_features_match(
    config: &ReleasePlzConfig,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> bool {
    config.packages.len() == intent.publish_features.len()
        && config.packages.iter().all(|package| {
            intent.publish_features.get(&package.name) == Some(&package.publish_features)
                && package.version_group
                    == intent.version_groups.iter().find_map(|(group, members)| {
                        members.contains(&package.name).then(|| group.clone())
                    })
        })
}

fn release_pin_matches(
    spec: &ReleaseWorkflowSpec,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> bool {
    spec.jobs.values().filter(|job| matches!(job.role, ReleaseRole::Preflight | ReleaseRole::Reconcile)).all(|job| {
        matches!(job.steps.iter().find(|step| step.role == Some(velnor_actions_contract::StepRole::AcquireVelnor)).map(|step| &step.kind), Some(velnor_actions_contract::StepKind::Shell { env, .. })
            if env.get("VELNOR_GENERATOR_SOURCE_SHA") == Some(&intent.generator_source_sha)
            && env.get("VELNOR_GENERATOR_MANIFEST_SHA256") == Some(&intent.generator_manifest_sha256))
    })
}
