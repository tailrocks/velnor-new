use super::*;

fn native_task(name: &str) -> velnor_actions_contract::ProposedTask {
    let group = velnor_actions_rust::TaskGroup {
        task_id: format!("stack/rust/{name}/build/default"),
        package_id: name.to_owned(),
        package_name: name.to_owned(),
        manifest_key: name.to_owned(),
        kind: velnor_actions_rust::TaskKind::Build,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: velnor_actions_rust::CompileDriver::Cargo,
        test_runner: velnor_actions_rust::TestRunner::CargoTest,
        nextest_profile: velnor_actions_rust::NextestProfile::Default,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    };
    let mut task = velnor_actions_rust::propose_task(&group).expect("fixture proposal");
    task.stack_id = velnor_actions_contract::Stack::Workload.id().to_owned();
    task.task_id = format!("stack/workload/{name}/install/node_ci");
    task.configuration = "node_ci".to_owned();
    task.identity.environment.insert(
        "VELNOR_NATIVE_NPM_SOURCE_CANDIDATES".to_owned(),
        serde_json::to_string(&[NativeNpmSource {
            name: "a".to_owned(),
            version: "1.2.3".to_owned(),
            resolved: "https://registry.npmjs.org/a/-/a-1.2.3.tgz".to_owned(),
            integrity: format!("sha512-{}==", "A".repeat(86)),
        }])
        .expect("candidate descriptor"),
    );
    task
}

fn consumer_jobs(tasks: &[velnor_actions_contract::ProposedTask]) -> BTreeMap<String, Job> {
    let grouped = crate::crate_job_ids::group_runnable(tasks);
    let assigned = crate::crate_job_ids::assign_group_ids(&grouped);
    assigned.into_values().map(|id| {
        let job = Job {
            cache_mode: None,
            display_name: id.clone(), runs_on: "ubuntu-26.04".to_owned(),
            timeout_minutes: velnor_actions_contract::JobTimeout::CRATE,
            needs: vec!["plan".to_owned()],
            condition: Some(format!("!cancelled() && needs.plan.result == 'success' && !contains(needs.plan.outputs.covered_tasks, ',{id},')")),
            permissions: None, environment: None, tool_producer: None, mbx_producer: None,
            source_producer: None, native_pages_deploy: None, native_publish: None, outputs: Vec::new(), steps: Vec::new(),
        };
        (id, job)
    }).collect()
}

#[test]
fn immutable_equal_source_groups_share_one_cohort_and_keep_both_admissions() {
    let tasks = vec![native_task("a"), native_task("b")];
    let mut jobs = consumer_jobs(&tasks);
    let cohorts = collect_cohorts(&jobs, &tasks, &ToolCatalog::pinned()).expect("cohorts");
    assert_eq!(cohorts.len(), 1);
    let cohort = cohorts.values().next().expect("shared cohort");
    assert_eq!(cohort.consumers.len(), 2);
    assert_eq!(
        cohort.selection.tasks,
        [tasks[0].task_id.clone(), tasks[1].task_id.clone()]
    );
    assert!(!cohort.selection.cargo_fallback);
    assert!(!cohort.selection.unconditional);
    jobs.values_mut().next().expect("consumer").runs_on = "ubuntu-24.04".to_owned();
    assert_eq!(
        collect_cohorts(&jobs, &tasks, &ToolCatalog::pinned())
            .expect("platform cohorts")
            .len(),
        2
    );
}

#[test]
fn missing_candidate_metadata_allocates_no_source_cohort() {
    let mut task = native_task("a");
    task.identity.environment.clear();
    let tasks = [task];
    let jobs = consumer_jobs(&tasks);
    assert!(
        collect_cohorts(&jobs, &tasks, &ToolCatalog::pinned())
            .expect("no candidates")
            .is_empty()
    );
    assert!(jobs.values().all(|job| job.needs == ["plan"]));
}

#[test]
fn unqualified_bun_consumer_emits_no_orphan_producer() {
    let mut task = native_task("a");
    task.configuration = "bun_ci".to_owned();
    task.task_id = "stack/workload/a/install/bun_ci".to_owned();
    let descriptor = task
        .identity
        .environment
        .remove("VELNOR_NATIVE_NPM_SOURCE_CANDIDATES")
        .expect("fixture descriptor");
    task.identity
        .environment
        .insert("VELNOR_NATIVE_BUN_SOURCE_CANDIDATES".to_owned(), descriptor);
    let tasks = [task];
    let jobs = consumer_jobs(&tasks);
    assert!(
        collect_cohorts(&jobs, &tasks, &ToolCatalog::pinned())
            .expect("Bun admission")
            .is_empty()
    );
}

#[test]
fn admitted_restore_uses_the_exact_plan_download_anchor_once() {
    let tasks = [native_task("a")];
    let mut jobs = consumer_jobs(&tasks);
    let cohorts = collect_cohorts(&jobs, &tasks, &ToolCatalog::pinned()).expect("cohorts");
    let cohort = cohorts.values().next().expect("cohort");
    let consumer = jobs.values_mut().next().expect("consumer");
    assert!(
        attach_restore(consumer, cohort)
            .is_err_and(|error| error.to_string().contains("consumer_source_anchor_missing"))
    );
    assert!(consumer.steps.is_empty());
    let anchor = crate::matrix_step::download_plan_step().expect("plan anchor");
    consumer.steps.push(anchor.clone());
    attach_restore(consumer, cohort).expect("admitted restore");
    assert_eq!(consumer.steps.first(), Some(&anchor));
    assert_eq!(
        consumer.steps.get(1),
        Some(&cache::npm_source_job::restore_step(&cohort.key).expect("exact restore"))
    );
    assert!(attach_restore(consumer, cohort).is_err_and(|error| {
        error
            .to_string()
            .contains("source_restore_already_attached")
    }));
}

#[test]
fn producer_admission_requires_trusted_push_plan_and_any_selected_consumer() {
    let first = "stack/workload/a/install/node_ci";
    let second = "stack/workload/b/build/node_ci";
    let selection = ToolProducerSelection {
        tasks: vec![first.to_owned(), second.to_owned()],
        cargo_fallback: false,
        unconditional: false,
    };
    selection.validate().expect("canonical source tasks");
    let condition = selection.condition(velnor_actions_contract::ToolCacheDomain::NpmBootstrap);
    assert!(condition.starts_with("!cancelled()"));
    assert!(condition.contains("needs.plan.result == 'success'"));
    assert!(
        condition.contains(velnor_actions_contract::workflow::cache_trust::CACHE_TRUSTED_PUSH_EXPR)
    );
    assert!(condition.contains(&format!("!contains(needs.plan.outputs.covered_tasks, ',{first},') || !contains(needs.plan.outputs.covered_tasks, ',{second},')")));
    assert_eq!(
        condition
            .matches("stack/workload/a/install/node_ci")
            .count(),
        1
    );
    assert!(!condition.contains("always()"));
}

#[test]
fn cohort_identity_preserves_cache_kind_runner_and_canonical_candidates() {
    let source = NativeNpmSource {
        name: "a".to_owned(),
        version: "1.2.3".to_owned(),
        resolved: "https://registry.npmjs.org/a/-/a-1.2.3.tgz".to_owned(),
        integrity: format!("sha512-{}==", "A".repeat(86)),
    };
    let catalog = ToolCatalog::pinned();
    let single = std::slice::from_ref(&source);
    let npm = source_identity(SourceProducerRole::Npm, single, &catalog, "ubuntu-26.04")
        .expect("npm identity");
    let duplicate = source_identity(
        SourceProducerRole::Npm,
        &[source.clone(), source.clone()],
        &catalog,
        "ubuntu-26.04",
    )
    .expect("canonical npm identity");
    assert_eq!(npm, duplicate);
    let bun = source_identity(SourceProducerRole::Bun, single, &catalog, "ubuntu-26.04")
        .expect("Bun identity");
    assert_ne!(npm, bun);
    let alternate = source_identity(SourceProducerRole::Npm, single, &catalog, "ubuntu-24.04")
        .expect("alternate runner");
    assert_ne!(npm, alternate);
}
