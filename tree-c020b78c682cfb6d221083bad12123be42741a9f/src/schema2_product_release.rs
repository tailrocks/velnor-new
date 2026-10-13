//! One serialized, exact-source coordinator with independent product DAGs.

use velnor_actions_contract::RoutingWorkflow;

use crate::yaml::Yaml;
use crate::{RenderError, runs_on::runs_on_yaml, steps::DOWNLOAD_ARTIFACT_USES};

use super::Schema2WorkflowRequest;
use super::features::CHECKOUT_USES;
use super::product_release_family::{self as family, Family};
use super::release_eligibility;

/// Emitted path for the single exact-source release coordinator.
pub(super) const WORKFLOW_PATH: &str = ".github/workflows/product-release.yml";
const HOSTED_RUNS_ON: &str = "ubuntu-26.04";
const RELEASE_CONCURRENCY: &str = "${{ github.repository }}-product-release";
const SCHEDULE_CRON: &str = "17 * * * *";
const SOURCE_OUTPUT: &str = "${{ needs.release-eligibility.outputs.source_sha }}";
const AUTHORITY_OUTPUT: &str = "${{ needs.release-eligibility.outputs.workflow_authority_sha }}";

/// Render the shared eligibility coordinator and requested independent families.
pub(super) fn render(request: &Schema2WorkflowRequest) -> Result<Option<Yaml>, RenderError> {
    let families = requested_families(request);
    if families.is_empty() {
        return Ok(None);
    }
    let hosted = runs_on_yaml(HOSTED_RUNS_ON)?;
    let mut jobs = vec![release_eligibility::job(hosted.clone())];
    for family in families {
        jobs.push(prepare_job(family, hosted.clone()));
        let family_document = family_document(family, request)?;
        for (id, job) in take_jobs(family_document)? {
            jobs.push(compose_job(id, job, family)?);
        }
    }
    Ok(Some(document(jobs)))
}

/// Select product families deterministically, independent of set iteration.
fn requested_families(request: &Schema2WorkflowRequest) -> Vec<Family> {
    [
        (RoutingWorkflow::ImageRelease, Family::Images),
        (RoutingWorkflow::MacosBinaryRelease, Family::Binary),
        (RoutingWorkflow::GeneratorRelease, Family::Generator),
    ]
    .into_iter()
    .filter_map(|(workflow, family)| request.workflows.contains(&workflow).then_some(family))
    .collect()
}

/// Render the existing family job graph; only the shared coordinator owns metadata.
fn family_document(family: Family, request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    match family {
        Family::Images => super::release::image_release(request),
        Family::Binary => super::release::macos_binary_release(request),
        Family::Generator => super::generator_release::generator_release(request),
    }
}

/// Extract only the typed job entries from a legacy family document.
fn take_jobs(document: Yaml) -> Result<Vec<(String, Yaml)>, RenderError> {
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
fn prepare_job(family: Family, hosted: Yaml) -> (String, Yaml) {
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
        ("run".to_owned(), Yaml::str(family::prepare_script(family))),
    ]);
    let mut fields = vec![
        (
            "name".to_owned(),
            Yaml::str(format!("Prepare {} release", family.label())),
        ),
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
    let mut steps = family::setup_steps();
    steps.push(output_step);
    fields.push(("steps".to_owned(), Yaml::Seq(steps)));
    (family.prepare_id().to_owned(), Yaml::Map(fields))
}

/// Rewire one existing build, attest, or publish job to the coordinator.
fn compose_job(id: String, job: Yaml, family: Family) -> Result<(String, Yaml), RenderError> {
    let (build_ids, attest_ids, publish_id) = family.job_ids();
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
    let needs = family_needs(&fields, role, family);
    let condition = job_condition(family, role);
    let env = Yaml::Map(vec![
        ("VELNOR_SOURCE_SHA".to_owned(), Yaml::str(SOURCE_OUTPUT)),
        (
            "VELNOR_WORKFLOW_AUTHORITY_SHA".to_owned(),
            Yaml::str(AUTHORITY_OUTPUT),
        ),
        (
            "VELNOR_CI_RUN_ID".to_owned(),
            Yaml::str("${{ needs.release-eligibility.outputs.ci_run_id }}"),
        ),
        (
            "VELNOR_CI_ATTEMPT".to_owned(),
            Yaml::str("${{ needs.release-eligibility.outputs.ci_attempt }}"),
        ),
    ]);
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
    for step in &mut steps {
        *step = compose_step(step.clone(), &id, role, family)?;
    }
    if matches!(role, JobRole::Publish) {
        steps.splice(0..0, family::setup_steps());
    }
    let mut output = fields
        .into_iter()
        .filter(|(key, _)| !matches!(key.as_str(), "needs" | "if" | "env" | "steps"))
        .collect::<Vec<_>>();
    if matches!(role, JobRole::Publish) {
        add_publish_attestation_permission(&mut output, &id)?;
    }
    output.push(("needs".to_owned(), needs));
    output.push(("if".to_owned(), Yaml::str(condition)));
    output.push(("env".to_owned(), env));
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

fn family_needs(fields: &[(String, Yaml)], role: JobRole, family: Family) -> Yaml {
    let (_, attest, _) = family.job_ids();
    let mut ids = vec![
        release_eligibility::JOB_ID.to_owned(),
        family.prepare_id().to_owned(),
    ];
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
    Yaml::Seq(ids.into_iter().map(Yaml::str).collect())
}

fn job_condition(family: Family, role: JobRole) -> String {
    let prepare = family.prepare_id();
    match role {
        JobRole::Build | JobRole::Attest => {
            format!("needs.{prepare}.outputs.action == 'build'")
        }
        JobRole::Publish => {
            let (_, attest_ids, _) = family.job_ids();
            let all_attestations = attest_ids
                .iter()
                .map(|id| format!("needs.{id}.result == 'success'"))
                .collect::<Vec<_>>()
                .join(" && ");
            format!(
                "always() && needs.{}.result == 'success' && needs.{prepare}.result == 'success' && (needs.{prepare}.outputs.action == 'complete' || ({all_attestations}))",
                release_eligibility::JOB_ID,
            )
        }
    }
}

/// Pin source checkout and replace only the existing family release command.
fn compose_step(
    step: Yaml,
    job_id: &str,
    role: JobRole,
    family: Family,
) -> Result<Yaml, RenderError> {
    let Yaml::Map(mut fields) = step else {
        return Ok(step);
    };
    let name = fields
        .iter()
        .find_map(|(key, value)| {
            (key == "name").then_some(match value {
                Yaml::Str(name) => Some(name.as_str()),
                _ => None,
            })
        })
        .flatten()
        .unwrap_or_default()
        .to_owned();
    if fields.iter().any(|(key, value)| {
        key == "uses" && matches!(value, Yaml::Str(uses) if uses == CHECKOUT_USES)
    }) {
        let Some((_, Yaml::Map(with))) = fields.iter_mut().find(|(key, _)| key == "with") else {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_checkout_inputs_missing:{job_id}"
            )));
        };
        with.retain(|(key, _)| key != "ref");
        with.push(("ref".to_owned(), Yaml::str(SOURCE_OUTPUT)));
    }
    if matches!(role, JobRole::Publish)
        && fields.iter().any(|(key, value)| {
            key == "uses" && matches!(value, Yaml::Str(uses) if uses == DOWNLOAD_ARTIFACT_USES)
        })
    {
        fields.retain(|(key, _)| key != "if");
        fields.push((
            "if".to_owned(),
            Yaml::str(format!(
                "needs.{}.outputs.action == 'build'",
                family.prepare_id()
            )),
        ));
    }
    if matches!(role, JobRole::Publish) && name == "Publish GitHub release" {
        let Some((_, run)) = fields.iter_mut().find(|(key, _)| key == "run") else {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_publish_command_missing:{job_id}"
            )));
        };
        *run = Yaml::str(family::publish_script(family));
        let Some((_, Yaml::Map(env))) = fields.iter_mut().find(|(key, _)| key == "env") else {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_publish_environment_missing:{job_id}"
            )));
        };
        env.push((
            "VELNOR_RELEASE_ACTION".to_owned(),
            Yaml::str(format!(
                "${{{{ needs.{}.outputs.action }}}}",
                family.prepare_id()
            )),
        ));
    }
    Ok(Yaml::Map(fields))
}

fn document(jobs: Vec<(String, Yaml)>) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Velnor product releases")),
        (
            "on".to_owned(),
            Yaml::Map(vec![
                (
                    "push".to_owned(),
                    Yaml::Map(vec![(
                        "branches".to_owned(),
                        Yaml::Seq(vec![Yaml::str("main")]),
                    )]),
                ),
                (
                    "schedule".to_owned(),
                    Yaml::Seq(vec![Yaml::Map(vec![(
                        "cron".to_owned(),
                        Yaml::str(SCHEDULE_CRON),
                    )])]),
                ),
                ("workflow_dispatch".to_owned(), Yaml::Map(Vec::new())),
            ]),
        ),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                ("group".to_owned(), Yaml::str(RELEASE_CONCURRENCY)),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ])
}

#[cfg(test)]
#[path = "schema2_product_release_tests.rs"]
mod tests;
