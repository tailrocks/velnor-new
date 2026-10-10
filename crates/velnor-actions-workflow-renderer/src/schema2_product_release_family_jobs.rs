//! Composition of image and binary product-release jobs.

use velnor_actions_contract::ReleaseTarget;

use crate::yaml::Yaml;
use crate::{RenderError, steps::DOWNLOAD_ARTIFACT_USES};

use super::super::features::CHECKOUT_USES;
use super::super::product_release_family::{self as family, Family};
use super::super::{
    ProductReleasePins, Schema2WorkflowRequest, generator_release, release, release_eligibility,
};
use super::{AUTHORITY_OUTPUT, MODULE_SOURCE, SOURCE_OUTPUT};

/// Render the existing family job graph; only the shared coordinator owns metadata.
pub(super) fn family_document(
    family: Family,
    request: &Schema2WorkflowRequest,
) -> Result<Yaml, RenderError> {
    match family {
        Family::Images => release::image_release(request),
        Family::Binary => release::macos_binary_release(request),
        Family::Generator => Err(RenderError::InvalidWorkflow(
            "generator_release_requires_typed_composition".to_owned(),
        )),
    }
}

/// Extract only the typed job entries from a legacy family document.
pub(super) fn take_jobs(document: Yaml) -> Result<Vec<(String, Yaml)>, RenderError> {
    let Yaml::Map(entries) = document else {
        return Err(RenderError::InvalidWorkflow(
            "release_family_document_not_map".to_owned(),
        ));
    };
    entries
        .into_iter()
        .find_map(|(key, value)| (key == "jobs").then_some(value))
        .and_then(|jobs| match jobs {
            Yaml::Map(jobs) => Some(jobs),
            _ => None,
        })
        .ok_or_else(|| RenderError::InvalidWorkflow("release_family_jobs_missing".to_owned()))
}

/// Create a read-only reconciliation job for one immutable family tag.
pub(super) fn prepare_job(
    family: Family,
    hosted: Yaml,
    request: &Schema2WorkflowRequest,
) -> Result<(String, Yaml), RenderError> {
    let pins = request
        .product_release
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("product_release_pins_missing".to_owned()))?;
    let prepare_script = family::prepare_script(family, pins)?;
    let output_step = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Reconcile immutable family release"),
        ),
        ("id".to_owned(), Yaml::str("reconcile")),
        (
            "env".to_owned(),
            Yaml::Map(vec![
                ("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")),
                ("VELNOR_SOURCE_SHA".to_owned(), Yaml::str(SOURCE_OUTPUT)),
                (
                    "VELNOR_WORKFLOW_AUTHORITY_SHA".to_owned(),
                    Yaml::str(AUTHORITY_OUTPUT),
                ),
            ]),
        ),
        ("run".to_owned(), Yaml::str(prepare_script)),
    ]);
    let mut fields = vec![
        (
            "name".to_owned(),
            Yaml::str(format!("Prepare {} release", family.label())),
        ),
        ("if".to_owned(), Yaml::str(family.selector_condition())),
        ("runs-on".to_owned(), hosted),
        ("timeout-minutes".to_owned(), Yaml::Int(30)),
        (
            "needs".to_owned(),
            Yaml::Seq(vec![Yaml::str(release_eligibility::JOB_ID)]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![
                ("attestations".to_owned(), Yaml::str("read")),
                ("actions".to_owned(), Yaml::str("read")),
                ("contents".to_owned(), Yaml::str("read")),
            ]),
        ),
        (
            "outputs".to_owned(),
            Yaml::Map(vec![(
                "action".to_owned(),
                Yaml::str("${{ steps.reconcile.outputs.action }}"),
            )]),
        ),
    ];
    let mut steps = Vec::new();
    if family == Family::Generator {
        steps.push(source_checkout_step());
    }
    steps.extend(generator_release::product_setup_steps(
        pins,
        ReleaseTarget::LinuxX86_64,
    )?);
    steps.push(output_step);
    fields.push(("steps".to_owned(), Yaml::Seq(steps)));
    Ok((family.prepare_id().to_owned(), Yaml::Map(fields)))
}

fn source_checkout_step() -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Checkout exact release source"),
        ),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("ref".to_owned(), Yaml::str(SOURCE_OUTPUT)),
                ("persist-credentials".to_owned(), Yaml::Bool(false)),
            ]),
        ),
    ])
}

/// Rewire one existing build, attest, or publish job to the coordinator.
pub(super) fn compose_job(
    id: String,
    job: Yaml,
    family: Family,
    pins: &ProductReleasePins,
) -> Result<(String, Yaml), RenderError> {
    let (build_ids, attest_ids, publish_id) = family.job_ids().ok_or_else(|| {
        RenderError::InvalidWorkflow("generator_family_requires_typed_composition".to_owned())
    })?;
    let role = if build_ids.contains(&id.as_str()) {
        JobRole::Build
    } else if attest_ids.contains(&id.as_str()) {
        JobRole::Attest
    } else if id == publish_id {
        JobRole::Publish
    } else {
        return Err(RenderError::InvalidWorkflow(format!(
            "unknown_release_family_job:{id}"
        )));
    };
    let Yaml::Map(fields) = job else {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_family_job_not_map:{id}"
        )));
    };
    let needs = family_needs(&fields, role, family)?;
    let condition = job_condition(family, role)?;
    let mut steps = fields
        .iter()
        .find_map(|(key, value)| (key == "steps").then_some(value.clone()))
        .and_then(|value| match value {
            Yaml::Seq(steps) => Some(steps),
            _ => None,
        })
        .ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("release_family_steps_missing:{id}"))
        })?;
    let publish_steps = steps
        .iter()
        .filter(|step| identified_step_role(step).is_some())
        .count();
    let expected_publish_steps = usize::from(matches!(role, JobRole::Publish));
    if publish_steps != expected_publish_steps {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_publish_step_count:{id}:{publish_steps}"
        )));
    }
    for step in &mut steps {
        let step_role = identified_step_role(step);
        *step = compose_step(step.clone(), &id, role, step_role, family, pins)?;
    }
    if matches!(role, JobRole::Publish) {
        steps.splice(
            0..0,
            generator_release::product_setup_steps(pins, family.runner_target())?,
        );
    }
    let mut output = fields
        .into_iter()
        .filter(|(key, _)| !matches!(key.as_str(), "needs" | "if" | "env" | "steps"))
        .collect::<Vec<_>>();
    if matches!(role, JobRole::Publish) {
        add_publish_attestation_permission(&mut output, &id)?;
    }
    if matches!(&needs, Yaml::Seq(entries) if !entries.is_empty()) {
        output.push(("needs".to_owned(), needs));
    }
    output.push(("if".to_owned(), Yaml::str(condition)));
    output.push(("steps".to_owned(), Yaml::Seq(steps)));
    Ok((id, Yaml::Map(output)))
}

fn add_publish_attestation_permission(
    fields: &mut [(String, Yaml)],
    job_id: &str,
) -> Result<(), RenderError> {
    let Some((_, Yaml::Map(permissions))) = fields.iter_mut().find(|(key, _)| key == "permissions")
    else {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_publish_permissions_missing:{job_id}"
        )));
    };
    permissions.retain(|(key, _)| key != "attestations");
    permissions.push(("attestations".to_owned(), Yaml::str("read")));
    Ok(())
}

#[derive(Clone, Copy)]
enum JobRole {
    Build,
    Attest,
    Publish,
}

fn family_needs(
    fields: &[(String, Yaml)],
    role: JobRole,
    family: Family,
) -> Result<Yaml, RenderError> {
    let Some((_, attest, _)) = family.job_ids() else {
        return Err(RenderError::InvalidWorkflow(
            "generator_family_requires_typed_composition".to_owned(),
        ));
    };
    let mut ids = vec!["verify-release-caller".to_owned()];
    if matches!(role, JobRole::Publish) {
        ids.extend(attest.iter().map(|id| (*id).to_owned()));
    }
    for (key, value) in fields {
        if key == "needs"
            && let Yaml::Seq(existing) = value
        {
            for item in existing {
                if let Yaml::Str(name) = item
                    && !ids.contains(name)
                {
                    ids.push(name.clone());
                }
            }
        }
    }
    Ok(Yaml::Seq(ids.into_iter().map(Yaml::str).collect()))
}

fn job_condition(family: Family, role: JobRole) -> Result<String, RenderError> {
    let Some((_, attest_ids, _)) = family.job_ids() else {
        return Err(RenderError::InvalidWorkflow(
            "generator_family_requires_typed_composition".to_owned(),
        ));
    };
    Ok(match role {
        JobRole::Build | JobRole::Attest => "inputs.release_action == 'build'".to_owned(),
        JobRole::Publish => {
            let all_attestations = attest_ids
                .iter()
                .map(|id| format!("needs.{id}.result == 'success'"))
                .collect::<Vec<_>>()
                .join(" && ");
            format!(
                "always() && (inputs.release_action == 'complete' || (inputs.release_action == 'build' && ({all_attestations})))",
            )
        }
    })
}

fn identified_step_role(step: &Yaml) -> Option<family::StepRole> {
    let Yaml::Map(fields) = step else {
        return None;
    };
    fields.iter().find_map(|(key, value)| {
        if key == "id"
            && let Yaml::Str(id) = value
        {
            return family::step_role(id);
        }
        None
    })
}

/// Pin source checkout and replace the step bound to the typed family role.
fn compose_step(
    step: Yaml,
    job_id: &str,
    role: JobRole,
    step_role: Option<family::StepRole>,
    family: Family,
    pins: &ProductReleasePins,
) -> Result<Yaml, RenderError> {
    let Yaml::Map(mut fields) = step else {
        return Ok(step);
    };
    if fields.iter().any(|(key, value)| {
        key == "uses" && matches!(value, Yaml::Str(uses) if uses == CHECKOUT_USES)
    }) {
        let Some((_, Yaml::Map(with))) = fields.iter_mut().find(|(key, _)| key == "with") else {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_checkout_inputs_missing:{job_id}"
            )));
        };
        with.retain(|(key, _)| key != "ref");
        with.push(("ref".to_owned(), Yaml::str(MODULE_SOURCE)));
    }
    if matches!(role, JobRole::Publish)
        && fields.iter().any(|(key, value)| {
            key == "uses" && matches!(value, Yaml::Str(uses) if uses == DOWNLOAD_ARTIFACT_USES)
        })
    {
        fields.retain(|(key, _)| key != "if");
        fields.push((
            "if".to_owned(),
            Yaml::str("inputs.release_action == 'build'"),
        ));
    }
    if matches!(step_role, Some(family::StepRole::Publish)) {
        if !matches!(role, JobRole::Publish) {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_publish_step_outside_publish_job:{job_id}"
            )));
        }
        let Some((_, run)) = fields.iter_mut().find(|(key, _)| key == "run") else {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_publish_command_missing:{job_id}"
            )));
        };
        *run = Yaml::str(family::publish_script(family, pins)?);
        let Some((_, Yaml::Map(env))) = fields.iter_mut().find(|(key, _)| key == "env") else {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_publish_environment_missing:{job_id}"
            )));
        };
        env.push((
            "VELNOR_RELEASE_ACTION".to_owned(),
            Yaml::str("${{ inputs.release_action }}"),
        ));
    }
    Ok(Yaml::Map(fields))
}

#[cfg(test)]
#[path = "schema2_product_release_family_jobs_tests.rs"]
mod tests;
