use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_contract::{
    ContractError, Job, ProposedTask, Stack, StepKind, matrix_id_for_task_group, matrix_key_for_id,
};
use velnor_actions_workflow_renderer::render::job_workflow_pair_is_relocatable;

use super::{JobOwners, group_runnable};

type TaskExecutionIdentity = (String, String, String);

/// Select finalized hosted and Scale Set jobs for lowerable Rust owner pairs.
///
/// Owners come from typed runnable proposals. A pair is returned only when
/// both exact lane jobs exist, carry that owner's task and matrix identities,
/// and pass the renderer's production lowering checks. Unsupported pairs stay
/// inline; jobs outside the proposal-derived owners are never selected.
///
/// # Errors
/// Returns an error for a partial lane pair or a task/matrix identity mismatch.
pub(crate) fn typed_rust_lane_job_ids_for_jobs(
    proposals: &[ProposedTask],
    jobs: &BTreeMap<String, Job>,
) -> Result<BTreeSet<String>, ContractError> {
    let grouped = group_runnable(proposals)?;
    let owners = JobOwners::from_grouped(proposals, &grouped);
    let mut selected = BTreeSet::new();
    let mut seen_task_ids = BTreeSet::new();

    for (group, members) in grouped
        .iter()
        .filter(|(group, _)| group.stack == Stack::Rust)
    {
        let owner = owners
            .group_id(group)
            .ok_or_else(|| rust_lane_error("group_owner_missing"))?;
        let hosted_id = format!("{owner}{HOSTED_SUFFIX}");
        let local_id = format!("{owner}{SCALE_SUFFIX}");
        let hosted = jobs.get(&hosted_id);
        let local = jobs.get(&local_id);

        let (Some(hosted), Some(local)) = (hosted, local) else {
            if hosted.is_none() && local.is_none() {
                continue;
            }
            return Err(rust_lane_error(format!("lane_pair_missing:{owner}")));
        };

        let expected = expected_rust_task_identities(members, &mut seen_task_ids)?;
        validate_lane_task_identities(&hosted_id, hosted, &expected)?;
        validate_lane_task_identities(&local_id, local, &expected)?;

        if job_workflow_pair_is_relocatable(&hosted_id, hosted, &local_id, local)
            && (!selected.insert(hosted_id) || !selected.insert(local_id))
        {
            return Err(rust_lane_error("duplicate_lane_job_id"));
        }
    }

    Ok(selected)
}

fn expected_rust_task_identities(
    members: &[&ProposedTask],
    seen_task_ids: &mut BTreeSet<String>,
) -> Result<BTreeSet<TaskExecutionIdentity>, ContractError> {
    let mut identities = BTreeSet::new();
    for member in members {
        if !seen_task_ids.insert(member.task_id.clone()) {
            return Err(rust_lane_error("duplicate_rust_task_id"));
        }
        let matrix_id = matrix_id_for_task_group(&member.stack_id, &member.task_id)?;
        let matrix_key = matrix_key_for_id(&matrix_id)?;
        identities.insert((member.task_id.clone(), matrix_id, matrix_key));
    }
    if identities.len() != members.len() {
        return Err(rust_lane_error("duplicate_group_task_identity"));
    }
    Ok(identities)
}

fn validate_lane_task_identities(
    job_id: &str,
    job: &Job,
    expected: &BTreeSet<TaskExecutionIdentity>,
) -> Result<(), ContractError> {
    let actual: Vec<_> = job
        .steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::TaskExecution {
                task_id,
                matrix_id,
                matrix_key,
                ..
            } => Some((task_id.clone(), matrix_id.clone(), matrix_key.clone())),
            _ => None,
        })
        .collect();
    let unique: BTreeSet<_> = actual.iter().cloned().collect();
    if actual.len() != expected.len() || unique != *expected {
        return Err(rust_lane_error(format!("task_identity_mismatch:{job_id}")));
    }
    Ok(())
}

fn rust_lane_error(problem: impl Into<String>) -> ContractError {
    ContractError::identity("typed_rust_lane_jobs", problem)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use velnor_actions_contract::cachekey::ToolchainInputs;
    use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
    use velnor_actions_contract::{
        Job, JobTimeout, ProposedTask, ServiceContainer, Stack, Step, StepKind, StepRole,
        TaskBindingDigest, matrix_id_for_task_group, matrix_key_for_id,
    };
    use velnor_actions_rust::{CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner};

    use super::typed_rust_lane_job_ids_for_jobs;
    use crate::crate_job_ids::{JobOwners, group_runnable};

    fn task(package: &str, kind: TaskKind) -> ProposedTask {
        velnor_actions_rust::propose_task(&TaskGroup {
            all_features: false,
            task_id: format!("stack/rust/{package}/{}/default", kind.as_str()),
            package_id: format!("{package} 0.1.0"),
            package_name: package.to_owned(),
            manifest_key: package.to_owned(),
            kind,
            configuration: "default".to_owned(),
            features: Vec::new(),
            target: "host".to_owned(),
            gated_by: Vec::new(),
            depends_on: Vec::new(),
            target_flags: Vec::new(),
            no_test_targets: false,
            package_arg: None,
            compile_driver: CompileDriver::Cargo,
            test_runner: TestRunner::CargoTest,
            nextest_profile: NextestProfile::Default,
            declared_inputs: Vec::new(),
            undeclared_reads: false,
            uses_network: false,
            uses_clock: false,
            uses_random: false,
            run_ignored: None,
        })
        .expect("proposal")
    }

    fn owner_and_members<'a>(
        proposals: &'a [ProposedTask],
        package: &str,
        stack: Stack,
    ) -> (String, Vec<&'a ProposedTask>) {
        let grouped = group_runnable(proposals).expect("groups");
        let key = grouped
            .keys()
            .find(|key| key.package_id == format!("{package} 0.1.0") && key.stack == stack)
            .expect("owner group");
        let members = grouped.get(key).expect("group members").clone();
        let owner = JobOwners::from_grouped(proposals, &grouped)
            .group_id(key)
            .expect("assigned owner")
            .clone();
        (owner, members)
    }

    fn pair_for_package(
        proposals: &[ProposedTask],
        package: &str,
        stack: Stack,
    ) -> (String, String, BTreeMap<String, Job>) {
        let (owner, members) = owner_and_members(proposals, package, stack);
        let hosted_id = format!("{owner}{HOSTED_SUFFIX}");
        let local_id = format!("{owner}{SCALE_SUFFIX}");
        let jobs = BTreeMap::from([
            (
                hosted_id.clone(),
                lane_job(&members, "ubuntu-26.04", vec!["plan".to_owned()]),
            ),
            (
                local_id.clone(),
                lane_job(
                    &members,
                    "scale-set:velnor+ubuntu-26.04-scale-set",
                    vec!["plan".to_owned()],
                ),
            ),
        ]);
        (hosted_id, local_id, jobs)
    }

    fn lane_job(members: &[&ProposedTask], runs_on: &str, needs: Vec<String>) -> Job {
        Job {
            display_name: "Rust / fixture".to_owned(),
            runs_on: runs_on.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::CRATE,
            needs,
            condition: None,
            permissions: None,
            cache_mode: None,
            environment: None,
            services: Vec::new(),
            steps: members.iter().map(|member| task_step(member)).collect(),
        }
    }

    fn task_step(member: &ProposedTask) -> Step {
        let matrix_id =
            matrix_id_for_task_group(&member.stack_id, &member.task_id).expect("matrix id");
        let matrix_key = matrix_key_for_id(&matrix_id).expect("matrix key");
        Step {
            name: "Run task".to_owned(),
            id: None,
            role: None,
            condition: None,
            parallel: false,
            kind: StepKind::TaskExecution {
                argv: vec!["cargo".to_owned(), "check".to_owned()],
                env: BTreeMap::new(),
                task_id: member.task_id.clone(),
                task_digest: TaskBindingDigest::new(format!("b3-{}", "a".repeat(64)))
                    .expect("task digest"),
                toolchain_inputs: ToolchainInputs {
                    tools: Vec::new(),
                    components: Vec::new(),
                    compile_driver: "cargo".to_owned(),
                    test_runner: "cargo_test".to_owned(),
                },
                matrix_id,
                matrix_key,
                report_helper_version: "1".to_owned(),
                matrix_max_parallel: None,
            },
        }
    }

    fn selected_pair(hosted_id: &str, local_id: &str) -> BTreeSet<String> {
        BTreeSet::from([hosted_id.to_owned(), local_id.to_owned()])
    }

    fn assert_inline(proposals: &[ProposedTask], jobs: &BTreeMap<String, Job>) {
        assert!(
            typed_rust_lane_job_ids_for_jobs(proposals, jobs)
                .expect("valid inline candidates")
                .is_empty()
        );
    }

    #[test]
    fn only_typed_rust_owner_pairs_are_selected() {
        let rust = task("demo", TaskKind::Clippy);
        let mut tofu = task("demo", TaskKind::Clippy);
        tofu.stack_id = Stack::Tofu.id().to_owned();
        tofu.task_kind = "validate".to_owned();
        tofu.task_id = "stack/tofu/demo/validate/default".to_owned();
        let proposals = vec![rust, tofu];
        let (hosted_id, local_id, mut jobs) = pair_for_package(&proposals, "demo", Stack::Rust);
        let (tofu_hosted, tofu_local, tofu_jobs) =
            pair_for_package(&proposals, "demo", Stack::Tofu);
        jobs.extend(tofu_jobs);
        jobs.insert("rust-forged__hosted".to_owned(), jobs[&hosted_id].clone());
        jobs.insert("rust-forged__local".to_owned(), jobs[&local_id].clone());

        assert_eq!(
            typed_rust_lane_job_ids_for_jobs(&proposals, &jobs).expect("typed owners"),
            selected_pair(&hosted_id, &local_id)
        );
        assert!(jobs.contains_key(&tofu_hosted));
        assert!(jobs.contains_key(&tofu_local));
    }

    #[test]
    fn empty_and_tofu_only_inventories_ignore_rust_looking_jobs() {
        let mut tofu = task("rust-demo", TaskKind::Clippy);
        tofu.stack_id = Stack::Tofu.id().to_owned();
        tofu.task_kind = "validate".to_owned();
        tofu.task_id = "stack/tofu/rust-demo/validate/default".to_owned();
        let (_, _, mut forged_jobs) = pair_for_package(&[tofu.clone()], "rust-demo", Stack::Tofu);
        let tofu_job = forged_jobs.values().next().expect("Tofu owner job").clone();
        forged_jobs.insert(format!("rust-rust-demo{HOSTED_SUFFIX}"), tofu_job.clone());
        forged_jobs.insert(format!("rust-rust-demo{SCALE_SUFFIX}"), tofu_job);

        assert_inline(&[], &forged_jobs);
        assert_inline(&[tofu], &forged_jobs);
    }

    #[test]
    fn no_pair_stays_inline_but_a_partial_pair_is_an_error() {
        let proposals = vec![task("demo", TaskKind::Clippy)];
        let (hosted_id, local_id, jobs) = pair_for_package(&proposals, "demo", Stack::Rust);
        let hosted = jobs.get(&hosted_id).expect("hosted lane").clone();

        assert_inline(&proposals, &BTreeMap::new());
        assert!(
            typed_rust_lane_job_ids_for_jobs(&proposals, &BTreeMap::from([(hosted_id, hosted)]))
                .is_err(),
            "a one-sided lane pair is incomplete"
        );
        assert!(jobs.contains_key(&local_id));
    }

    #[test]
    fn task_matrix_and_group_identity_must_match_the_owner_proposals() {
        let proposals = vec![
            task("demo", TaskKind::Clippy),
            task("other", TaskKind::Clippy),
        ];
        let (hosted_id, local_id, jobs) = pair_for_package(&proposals, "demo", Stack::Rust);

        let mut wrong_task = jobs.clone();
        let StepKind::TaskExecution { task_id, .. } =
            &mut wrong_task.get_mut(&hosted_id).expect("hosted job").steps[0].kind
        else {
            panic!("task step")
        };
        *task_id = "absent-source-task".to_owned();
        assert!(typed_rust_lane_job_ids_for_jobs(&proposals, &wrong_task).is_err());

        let mut wrong_matrix_id = jobs.clone();
        let StepKind::TaskExecution { matrix_id, .. } = &mut wrong_matrix_id
            .get_mut(&hosted_id)
            .expect("hosted job")
            .steps[0]
            .kind
        else {
            panic!("task step")
        };
        matrix_id.push_str("-wrong");
        assert!(typed_rust_lane_job_ids_for_jobs(&proposals, &wrong_matrix_id).is_err());

        let mut wrong_matrix_key = jobs.clone();
        let StepKind::TaskExecution { matrix_key, .. } = &mut wrong_matrix_key
            .get_mut(&hosted_id)
            .expect("hosted job")
            .steps[0]
            .kind
        else {
            panic!("task step")
        };
        matrix_key.push_str("-wrong");
        assert!(typed_rust_lane_job_ids_for_jobs(&proposals, &wrong_matrix_key).is_err());

        let mut extra_group_task = jobs;
        extra_group_task
            .get_mut(&hosted_id)
            .expect("hosted job")
            .steps
            .push(task_step(&proposals[1]));
        assert!(typed_rust_lane_job_ids_for_jobs(&proposals, &extra_group_task).is_err());
        assert!(extra_group_task.contains_key(&local_id));
    }

    #[test]
    fn unsupported_dependency_shapes_leave_both_lanes_inline() {
        let proposals = vec![task("demo", TaskKind::Clippy)];
        let (_, _, mut jobs) = pair_for_package(&proposals, "demo", Stack::Rust);
        for job in jobs.values_mut() {
            job.needs.push("dependency-seed".to_owned());
        }
        assert_inline(&proposals, &jobs);

        let mut service_jobs = pair_for_package(&proposals, "demo", Stack::Rust).2;
        let service = ServiceContainer::postgres(18).expect("catalog service");
        for job in service_jobs.values_mut() {
            job.services.push(service.clone());
        }
        assert_inline(&proposals, &service_jobs);

        let mut seed_jobs = pair_for_package(&proposals, "demo", Stack::Rust).2;
        for job in seed_jobs.values_mut() {
            job.steps.push(Step {
                name: "Build dependency seed".to_owned(),
                id: None,
                role: Some(StepRole::MbxSeedBuild),
                condition: None,
                parallel: false,
                kind: StepKind::Shell {
                    run: vec!["true".to_owned()],
                    env: BTreeMap::new(),
                },
            });
        }
        assert_inline(&proposals, &seed_jobs);
    }
}
