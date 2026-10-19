//! Tofu provider-cache render tests (T21).
//!
//! Declared via `#[path]` from `crate_jobs.rs` under `cfg(test)`.

use super::*;
use velnor_actions_contract::StepKind;
use velnor_actions_rust::TaskKind;

/// Tofu proposal via the T12 adapter constructor.
fn tofu_group(root: &str, kind: velnor_actions_tofu::TofuTaskKind) -> ProposedTask {
    let group = velnor_actions_tofu::TofuTaskGroup {
        root: root.to_owned(),
        kind,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = velnor_actions_tofu::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

/// One fmt/init/validate triple per root, all runnable.
fn tofu_triples(roots: &[&str]) -> Vec<ProposedTask> {
    use velnor_actions_tofu::TofuTaskKind;
    let mut tasks = Vec::new();
    for root in roots {
        for kind in [
            TofuTaskKind::Fmt,
            TofuTaskKind::InitForValidate,
            TofuTaskKind::Validate,
        ] {
            tasks.push(tofu_group(root, kind));
        }
    }
    tasks
}

/// Action inputs of one named step.
fn step_inputs<'a>(job: &'a Job, name: &str) -> &'a std::collections::BTreeMap<String, String> {
    let step = job
        .steps
        .iter()
        .find(|step| step.name == name)
        .expect("named step");
    let StepKind::Action { with, .. } = &step.kind else {
        panic!("{name} must be an action step");
    };
    with
}

#[test]
fn provider_restore_precedes_init_obligation() {
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &crate_jobs_tests::discovery(tofu_triples(&["stacks/a"])),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        seed_parallel_targets: false,
        crates: &std::collections::BTreeMap::new(),
    })
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1, "one job per tofu root");
    let job = &found.jobs[0].1;
    let names = crate_jobs_tests::names(job);
    let restore_at = names
        .iter()
        .position(|step| *step == "Restore Tofu providers")
        .expect("provider restore present");
    let init_at = names
        .iter()
        .position(|step| *step == "Init for validate")
        .expect("init present");
    let validate_at = names
        .iter()
        .position(|step| *step == "Validate")
        .expect("validate present");
    assert!(restore_at < init_at, "restore before init: {names:?}");
    assert!(init_at < validate_at, "init gates validate: {names:?}");
    let with = step_inputs(job, "Restore Tofu providers");
    let key = with.get("cache-key").expect("restore key");
    assert_eq!(key, "${{needs.plan.outputs.tofu_providers_key_0}}");
}

#[test]
fn provider_restore_keys_are_per_root() {
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &crate_jobs_tests::discovery(tofu_triples(&["stacks/a", "stacks/b"])),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 5,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        seed_parallel_targets: false,
        crates: &std::collections::BTreeMap::new(),
    })
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 2);
    let mut keys: Vec<&str> = found
        .jobs
        .iter()
        .map(|(_, job)| {
            step_inputs(job, "Restore Tofu providers")
                .get("cache-key")
                .expect("restore key")
                .as_str()
        })
        .collect();
    keys.sort_unstable();
    assert_eq!(keys.len(), 2);
    assert_ne!(keys[0], keys[1], "each root owns its key");
    assert_eq!(
        keys,
        [
            "${{needs.plan.outputs.tofu_providers_key_0}}",
            "${{needs.plan.outputs.tofu_providers_key_1}}"
        ],
        "each root reads its own Plan key ordinal"
    );
}

#[test]
fn rust_jobs_carry_no_provider_restore() {
    let rust = crate_jobs_tests::group("demo", TaskKind::Clippy, &[]);
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &crate_jobs_tests::discovery(vec![rust]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[String::new()],
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        seed_parallel_targets: false,
        crates: &std::collections::BTreeMap::new(),
    })
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1);
    let names = crate_jobs_tests::names(&found.jobs[0].1);
    assert!(
        !names.contains(&"Restore Tofu providers"),
        "rust jobs restore no providers: {names:?}"
    );
}

#[test]
fn mixed_job_restores_both_sources_and_providers() {
    use velnor_actions_tofu::TofuTaskKind;
    let mut rust = crate_jobs_tests::group("demo", TaskKind::Clippy, &[]);
    let tofu = tofu_group("stacks/a", TofuTaskKind::Validate);
    rust.identity.unit_id.clone_from(&tofu.identity.unit_id);
    rust.configuration.clone_from(&tofu.configuration);
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &crate_jobs_tests::discovery(vec![rust, tofu]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[String::new()],
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        seed_parallel_targets: false,
        crates: &std::collections::BTreeMap::new(),
    })
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 2, "each stack owns its execution job");
    let rust = &found
        .jobs
        .iter()
        .find(|(id, _)| id.starts_with("rust-"))
        .expect("Rust owner")
        .1;
    let tofu = &found
        .jobs
        .iter()
        .find(|(id, _)| id.starts_with("tofu-"))
        .expect("Tofu owner")
        .1;
    let rust_names = crate_jobs_tests::names(rust);
    let tofu_names = crate_jobs_tests::names(tofu);
    assert!(
        rust_names.contains(&"Restore Cargo sources"),
        "{rust_names:?}"
    );
    assert!(
        !rust_names.contains(&"Restore Tofu providers"),
        "{rust_names:?}"
    );
    assert!(
        tofu_names.contains(&"Restore Tofu providers"),
        "{tofu_names:?}"
    );
    assert!(
        !tofu_names.contains(&"Restore Cargo sources"),
        "{tofu_names:?}"
    );
    assert!(!rust_names.contains(&"Validate"), "no foreign obligation");
    assert!(!tofu_names.contains(&"Clippy"), "no foreign obligation");
    for (names, restore, obligation) in [
        (&rust_names, "Restore Cargo sources", "Clippy"),
        (&tofu_names, "Restore Tofu providers", "Validate"),
    ] {
        let restore_at = names
            .iter()
            .position(|step| *step == restore)
            .expect("restore");
        let obligation_at = names
            .iter()
            .position(|step| *step == obligation)
            .expect("obligation");
        assert!(
            restore_at < obligation_at,
            "restore precedes its owned obligation: {names:?}"
        );
    }
}

#[test]
fn provider_keys_reject_unsafe_roots() {
    for root in [
        "../escape",
        "a/../b",
        "/abs",
        "a'b",
        "a\"b",
        "a$b",
        "a`b",
        "a\\b",
        "a\nb",
    ] {
        let err = crate::cache_keys::cache_keys_step(
            &velnor_actions_mise::ToolCatalog::pinned(),
            &[],
            &[root.to_owned()],
        )
        .expect_err("unsafe roots fail closed");
        assert!(
            err.to_string().contains("unsafe_fetch_root"),
            "{root:?}: {err}"
        );
    }
}

#[test]
fn root_obligations_reject_missing_and_extra_obligations() {
    let tasks = tofu_triples(&["stacks/a"]);
    let refs: Vec<_> = tasks.iter().collect();
    let obligations = obligations_for(&refs, &ToolCatalog::pinned()).expect("obligations");
    assert_eq!(
        crate::tofu_cache::tofu_root_for_obligations(&obligations).expect("one root"),
        "stacks/a"
    );

    let empty = crate::tofu_cache::tofu_root_for_obligations(&[])
        .expect_err("missing Tofu obligations fail closed");
    assert!(
        empty.to_string().contains("tofu_empty_obligations"),
        "{empty}"
    );

    let partial: Vec<_> = tasks
        .iter()
        .filter(|task| task.task_kind != "init")
        .collect();
    let obligations = obligations_for(&partial, &ToolCatalog::pinned()).expect("partial");
    assert_eq!(
        crate::tofu_cache::tofu_root_for_obligations(&obligations)
            .expect("omitted init keeps the exact root"),
        "stacks/a",
        "caller closure may omit non-runnable obligations"
    );

    let mut duplicate = obligations.clone();
    duplicate.push(obligations[0].clone());
    let err = crate::tofu_cache::tofu_root_for_obligations(&duplicate)
        .expect_err("duplicate same-root obligations fail closed");
    assert!(
        err.to_string().contains("tofu_duplicate_obligation"),
        "{err}"
    );

    let extra = tofu_triples(&["", "root"]);
    let refs: Vec<_> = extra.iter().collect();
    let obligations = obligations_for(&refs, &ToolCatalog::pinned()).expect("extra roots");
    let err = crate::tofu_cache::tofu_root_for_obligations(&obligations)
        .expect_err("different exact roots cannot share a provider cache");
    assert!(err.to_string().contains("tofu_mixed_roots"), "{err}");
}
