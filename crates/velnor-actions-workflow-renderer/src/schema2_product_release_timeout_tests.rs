use std::collections::BTreeSet;
use std::error::Error;

use velnor_actions_contract::RoutingWorkflow;

use crate::yaml::Yaml;

use super::product;

#[test]
fn compiler_image_and_homebrew_jobs_get_their_bounded_budgets() -> Result<(), Box<dyn Error>> {
    let product = product(&[
        RoutingWorkflow::ImageRelease,
        RoutingWorkflow::MacosBinaryRelease,
        RoutingWorkflow::GeneratorRelease,
    ])?;
    let mut workflows = vec![&product.workflow];
    workflows.extend(
        product
            .family_workflows
            .iter()
            .map(|(_, workflow)| workflow),
    );
    let observed = audit_workflows(&workflows).expect("generated timeout contract");
    let compiler_jobs = BTreeSet::from([
        "build-linux".to_owned(),
        "build-macos".to_owned(),
        "build-macos-intel".to_owned(),
        "build-binary".to_owned(),
    ]);
    let actual_compiler_jobs = observed
        .iter()
        .filter(|(job, _)| compiler_jobs.contains(job))
        .map(|(job, _)| job.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual_compiler_jobs, compiler_jobs);
    assert_eq!(
        observed
            .iter()
            .filter(|(job, _)| compiler_jobs.contains(job))
            .count(),
        4,
        "four compiler jobs must receive the ten-minute budget"
    );
    assert_eq!(
        observed.iter().find(|(job, _)| job == "build-images"),
        Some(&("build-images".to_owned(), 30)),
        "the dedicated image budget applies only to build-images"
    );
    for (job, timeout) in observed {
        let expected = if job == "build-images" || job == "qualify-homebrew-macos" {
            30
        } else if compiler_jobs.contains(&job)
            || matches!(
                job.as_str(),
                "qualify-linux" | "qualify-macos" | "qualify-macos-intel"
            )
        {
            10
        } else {
            5
        };
        assert_eq!(timeout, expected, "unexpected timeout for {job}");
    }
    Ok(())
}

#[test]
fn timeout_audit_rejects_missing_concrete_timeout_and_timeout_on_caller()
-> Result<(), Box<dyn Error>> {
    let product = product(&[
        RoutingWorkflow::ImageRelease,
        RoutingWorkflow::MacosBinaryRelease,
        RoutingWorkflow::GeneratorRelease,
    ])?;
    let mut missing = product.workflow.clone();
    mutate_job(&mut missing, "release-eligibility", |fields| {
        fields.retain(|(key, _)| key != "timeout-minutes");
    });
    assert!(audit_workflows(&[&missing]).is_err());

    let mut caller_timeout = product.workflow.clone();
    mutate_job(&mut caller_timeout, "release-images", |fields| {
        fields.push(("timeout-minutes".to_owned(), Yaml::Int(5)));
    });
    assert!(audit_workflows(&[&caller_timeout]).is_err());
    Ok(())
}

fn audit_workflows(workflows: &[&Yaml]) -> Result<Vec<(String, i64)>, String> {
    let mut observed = Vec::new();
    for workflow in workflows {
        let Yaml::Map(document) = workflow else {
            return Err("workflow is not a map".to_owned());
        };
        let jobs = document
            .iter()
            .find_map(|(key, value)| (key == "jobs").then_some(value))
            .ok_or_else(|| "workflow jobs missing".to_owned())?;
        let Yaml::Map(jobs) = jobs else {
            return Err("workflow jobs is not a map".to_owned());
        };
        for (id, job) in jobs {
            let Yaml::Map(fields) = job else {
                return Err(format!("job {id} is not a map"));
            };
            let has_steps = fields.iter().any(|(key, _)| key == "steps");
            let has_callee = fields.iter().any(|(key, _)| key == "uses");
            let timeout = fields
                .iter()
                .find_map(|(key, value)| (key == "timeout-minutes").then_some(value));
            match (has_steps, has_callee, timeout) {
                (true, false, Some(Yaml::Int(minutes))) => observed.push((id.clone(), *minutes)),
                (true, false, None) => return Err(format!("concrete job {id} has no timeout")),
                (true, false, Some(_)) => {
                    return Err(format!("concrete job {id} timeout is not integer"));
                }
                (false, true, None) => {}
                (false, true, Some(_)) => return Err(format!("caller job {id} has a timeout")),
                _ => return Err(format!("job {id} is neither concrete nor a caller")),
            }
        }
    }
    Ok(observed)
}

fn mutate_job(workflow: &mut Yaml, target: &str, mutate: impl FnOnce(&mut Vec<(String, Yaml)>)) {
    let Yaml::Map(document) = workflow else {
        panic!("workflow map");
    };
    let jobs = document
        .iter_mut()
        .find_map(|(key, value)| (key == "jobs").then_some(value))
        .expect("jobs field");
    let Yaml::Map(jobs) = jobs else {
        panic!("jobs map");
    };
    let job = jobs
        .iter_mut()
        .find_map(|(id, job)| (id == target).then_some(job))
        .expect("target job");
    let Yaml::Map(fields) = job else {
        panic!("job map");
    };
    mutate(fields);
}
