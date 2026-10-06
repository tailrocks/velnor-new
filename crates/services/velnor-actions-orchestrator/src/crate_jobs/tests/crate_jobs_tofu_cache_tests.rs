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
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &super::discovery(tofu_triples(&["stacks/a"])),
        &ToolCatalog::pinned(),
        &[],
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1, "one job per tofu root");
    let job = &found.jobs[0].1;
    let names = super::names(job);
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
    assert!(
        key.starts_with("velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-stacks-a-"),
        "{key}"
    );
    assert!(
        key.ends_with("${{hashFiles('stacks/a/.terraform.lock.hcl')}}"),
        "{key}"
    );
}

#[test]
fn provider_restore_keys_are_per_root() {
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &super::discovery(tofu_triples(&["stacks/a", "stacks/b"])),
        &ToolCatalog::pinned(),
        &[],
        None,
        5,
    )
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
    assert!(
        keys.iter()
            .any(|key| key.ends_with("${{hashFiles('stacks/a/.terraform.lock.hcl')}}")),
        "{keys:?}"
    );
    assert!(
        keys.iter()
            .any(|key| key.ends_with("${{hashFiles('stacks/b/.terraform.lock.hcl')}}")),
        "{keys:?}"
    );
}

#[test]
fn rust_jobs_carry_no_provider_restore() {
    let rust = super::group("demo", TaskKind::Clippy, &[]);
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &super::discovery(vec![rust]),
        &ToolCatalog::pinned(),
        &[String::new()],
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1);
    let names = super::names(&found.jobs[0].1);
    assert!(
        !names.contains(&"Restore Tofu providers"),
        "rust jobs restore no providers: {names:?}"
    );
}

#[test]
fn mixed_job_restores_both_sources_and_providers() {
    use velnor_actions_tofu::TofuTaskKind;
    let mut rust = super::group("demo", TaskKind::Clippy, &[]);
    let tofu = tofu_group("stacks/a", TofuTaskKind::Validate);
    rust.identity.unit_id.clone_from(&tofu.identity.unit_id);
    rust.configuration.clone_from(&tofu.configuration);
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &super::discovery(vec![rust, tofu]),
        &ToolCatalog::pinned(),
        &[String::new()],
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1, "shared group renders once");
    let names = super::names(&found.jobs[0].1);
    assert!(
        names.contains(&"Restore Cargo sources") || names.contains(&"Restore Cargo registry"),
        "mixed restores sources: {names:?}"
    );
    assert!(
        names.contains(&"Restore Tofu providers"),
        "mixed restores providers: {names:?}"
    );
    let restore_at = names
        .iter()
        .position(|step| *step == "Restore Tofu providers")
        .expect("provider restore present");
    let validate_at = names
        .iter()
        .position(|step| *step == "Validate")
        .expect("validate present");
    assert!(
        restore_at < validate_at,
        "provider restore precedes obligations: {names:?}"
    );
}

#[test]
fn provider_key_rejects_unknown_targets() {
    let err =
        crate::tofu_cache::tofu_providers_cache_key("mips-unknown-linux", "1.13.1", "stacks/a")
            .expect_err("unknown targets fail closed");
    assert!(err.to_string().contains("bad_target"), "unexpected {err:?}");
}

#[test]
fn provider_key_rejects_loose_tofu_versions() {
    for version in ["1.13", "latest", ">= 1.0", ""] {
        assert!(
            crate::tofu_cache::tofu_providers_cache_key(
                "x86_64-unknown-linux-gnu",
                version,
                "stacks/a"
            )
            .is_err(),
            "{version:?} must fail closed"
        );
    }
    assert!(
        crate::tofu_cache::tofu_providers_cache_key(
            "x86_64-unknown-linux-gnu",
            "1.13.1",
            "stacks/a"
        )
        .is_ok(),
        "the qualified pin builds"
    );
}

#[test]
fn provider_key_rejects_unsafe_roots() {
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
        let err =
            crate::tofu_cache::tofu_providers_cache_key("x86_64-unknown-linux-gnu", "1.13.1", root)
                .expect_err("unsafe roots fail closed");
        assert!(
            err.to_string().contains("unsafe_fetch_root"),
            "{root:?}: {err}"
        );
    }
}

#[test]
fn provider_key_rejects_leading_dash_roots() {
    for root in ["-evil", "-chdir"] {
        let err =
            crate::tofu_cache::tofu_providers_cache_key("x86_64-unknown-linux-gnu", "1.13.1", root)
                .expect_err("leading-dash roots fail closed");
        assert!(
            err.to_string().contains("leading_dash_root"),
            "{root:?}: {err}"
        );
    }
}

#[test]
fn provider_key_rejects_overlong_keys_from_deep_roots() {
    let deep = format!("{}leaf", "nest/".repeat(200));
    let err =
        crate::tofu_cache::tofu_providers_cache_key("x86_64-unknown-linux-gnu", "1.13.1", &deep)
            .expect_err("deep roots overflow the key");
    assert!(err.to_string().contains("key_too_long"), "{err}");
    let roomy = format!("{}leaf", "nest/".repeat(20));
    assert!(
        crate::tofu_cache::tofu_providers_cache_key("x86_64-unknown-linux-gnu", "1.13.1", &roomy,)
            .is_ok(),
        "ordinary nesting builds"
    );
}
