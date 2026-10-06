//! Typed composition of the source-bound generator release graph.

use velnor_actions_contract::ReleaseTarget;

use crate::RenderError;
use crate::yaml::Yaml;

use super::super::features::CHECKOUT_USES;
use super::super::generator_release::{self, JobRole};
use super::family_jobs::take_jobs;

/// Attach the shared source/eligibility gate to every typed generator job.
pub(super) fn compose_jobs(document: Yaml) -> Result<Vec<(String, Yaml)>, RenderError> {
    take_jobs(document)?
        .into_iter()
        .map(|(id, job)| compose_job(id, job))
        .collect()
}

fn compose_job(id: String, job: Yaml) -> Result<(String, Yaml), RenderError> {
    let role = generator_release::job_role(&id).ok_or_else(|| {
        RenderError::InvalidWorkflow(format!("unknown_typed_generator_release_job:{id}"))
    })?;
    let Yaml::Map(fields) = job else {
        return Err(RenderError::InvalidWorkflow(format!(
            "generator_release_job_not_map:{id}"
        )));
    };
    let needs = merge_needs(&fields, role);
    let env = merge_env(&fields, role)?;
    let mut steps = job_steps(&fields, &id)?;
    let mut output = fields
        .into_iter()
        .filter(|(key, _)| !matches!(key.as_str(), "needs" | "if" | "env" | "steps"))
        .collect::<Vec<_>>();
    for step in &mut steps {
        *step = bind_checkout(step.clone(), &id)?;
    }
    output.push(("needs".to_owned(), needs));
    output.push(("if".to_owned(), Yaml::str(condition(role))));
    if matches!(&env, Yaml::Map(entries) if !entries.is_empty()) {
        output.push(("env".to_owned(), env));
    }
    output.push(("steps".to_owned(), Yaml::Seq(steps)));
    Ok((id, Yaml::Map(output)))
}

fn merge_needs(fields: &[(String, Yaml)], role: JobRole) -> Yaml {
    let mut ids = vec!["verify-release-caller".to_owned()];
    if matches!(role, JobRole::Publish | JobRole::AttestManifest) {
        ids.extend(["attest-linux", "attest-macos", "attest-macos-intel"].map(str::to_owned));
    }
    if matches!(role, JobRole::Publish) {
        ids.push("attest-manifest".to_owned());
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

fn condition(role: JobRole) -> String {
    match role {
        JobRole::Publish => "always() && inputs.release_action == 'build' && needs.attest-linux.result == 'success' && needs.attest-macos.result == 'success' && needs.attest-macos-intel.result == 'success' && needs.attest-manifest.result == 'success'".to_owned(),
        JobRole::SourceGate
        | JobRole::Build(_)
        | JobRole::CandidateManifest
        | JobRole::Qualify(_)
        | JobRole::Attest(_)
        | JobRole::AttestManifest
        => "inputs.release_action == 'build'".to_owned(),
    }
}

fn merge_env(fields: &[(String, Yaml)], role: JobRole) -> Result<Yaml, RenderError> {
    let existing = fields
        .iter()
        .find(|(key, _)| key == "env")
        .map(|(_, value)| value);
    let mut env = match existing {
        Some(Yaml::Map(entries)) => entries.clone(),
        Some(_) => {
            return Err(RenderError::InvalidWorkflow(
                "generator_release_job_env_not_map".to_owned(),
            ));
        }
        None => Vec::new(),
    };
    if let Some(target) = target_for_role(role) {
        env.retain(|(key, _)| key != "VELNOR_RELEASE_TARGET");
        env.push((
            "VELNOR_RELEASE_TARGET".to_owned(),
            Yaml::str(target.triple()),
        ));
    }
    Ok(Yaml::Map(env))
}

fn target_for_role(role: JobRole) -> Option<ReleaseTarget> {
    match role {
        JobRole::Build(target) | JobRole::Qualify(target) => Some(target),
        JobRole::SourceGate
        | JobRole::Attest(_)
        | JobRole::CandidateManifest
        | JobRole::AttestManifest
        | JobRole::Publish => None,
    }
}

fn job_steps(fields: &[(String, Yaml)], id: &str) -> Result<Vec<Yaml>, RenderError> {
    fields
        .iter()
        .find_map(|(key, value)| (key == "steps").then_some(value))
        .and_then(|value| match value {
            Yaml::Seq(steps) => Some(steps.clone()),
            _ => None,
        })
        .ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("generator_release_steps_missing:{id}"))
        })
}

fn bind_checkout(step: Yaml, job_id: &str) -> Result<Yaml, RenderError> {
    let Yaml::Map(mut fields) = step else {
        return Ok(step);
    };
    let is_checkout = fields.iter().any(|(key, value)| {
        key == "uses" && matches!(value, Yaml::Str(uses) if uses == CHECKOUT_USES)
    });
    if !is_checkout {
        return Ok(Yaml::Map(fields));
    }
    let Some((_, Yaml::Map(with))) = fields.iter_mut().find(|(key, _)| key == "with") else {
        return Err(RenderError::InvalidWorkflow(format!(
            "generator_release_checkout_inputs_missing:{job_id}"
        )));
    };
    with.retain(|(key, _)| key != "ref");
    with.push(("ref".to_owned(), Yaml::str("${{ inputs.source_sha }}")));
    Ok(Yaml::Map(fields))
}

#[cfg(test)]
#[path = "schema2_product_release_generator_tests.rs"]
mod tests;
