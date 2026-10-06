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
    let mut task = velnor_actions_tofu::propose_task(&group).expect("fixture proposes");
    task.identity.environment.insert(
        crate::select_tofu::PUBLIC_PROVIDER_TRANSPORT.to_owned(),
        "true".to_owned(),
    );
    let descriptor = crate::tofu_cache_source::ProviderExportDescriptor {
        root: root.to_owned(),
        selections: vec![("registry.opentofu.org/hashicorp/random".to_owned(), "3.6.3".to_owned())],
        lock_content: "provider \"registry.opentofu.org/hashicorp/random\" {\n  version = \"3.6.3\"\n  hashes = [\"h1:fixture\"]\n}\n".to_owned(),
    };
    task.identity.environment.insert(
        crate::select_tofu::TOFU_PROVIDER_EXPORT.to_owned(),
        serde_json::to_string(&descriptor).expect("fixture descriptor"),
    );
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
        &crate_jobs_tests::discovery(tofu_triples(&["stacks/a"])),
        &ToolCatalog::pinned(),
        &[],
        &[],
        None,
        2,
    )
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
    let key = with.get("key").expect("restore key");
    assert!(
        key.starts_with("velnor-v2-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-b3-"),
        "{key}"
    );
    assert!(
        !key.contains("${{")
            && key
                .rsplit('-')
                .next()
                .is_some_and(|digest| digest.len() == 64),
        "{key}"
    );
}

#[test]
fn unqualified_provider_roots_keep_validation_without_transport() {
    for value in [None, Some("false")] {
        let mut tasks = tofu_triples(&["stacks/private"]);
        for task in &mut tasks {
            task.identity
                .environment
                .remove(crate::select_tofu::PUBLIC_PROVIDER_TRANSPORT);
            if let Some(value) = value {
                task.identity.environment.insert(
                    crate::select_tofu::PUBLIC_PROVIDER_TRANSPORT.to_owned(),
                    value.to_owned(),
                );
            }
        }
        let found = build_crate_jobs(
            "ubuntu-26.04",
            WorkflowPolicy::ConsumerV1,
            &crate_jobs_tests::discovery(tasks),
            &ToolCatalog::pinned(),
            &[],
            &[],
            None,
            2,
        )
        .expect("private root still builds");
        let names = crate_jobs_tests::names(&found.jobs[0].1);
        assert!(!names.contains(&"Restore Tofu providers"));
        assert!(!names.contains(&"Save Tofu providers"));
        assert!(names.contains(&"Prepare isolated Tofu configuration"));
        assert!(names.contains(&"Init for validate"));
        assert!(names.contains(&"Validate"));
    }
}

#[test]
fn provider_restore_keys_are_per_root() {
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &crate_jobs_tests::discovery(tofu_triples(&["stacks/a", "stacks/b"])),
        &ToolCatalog::pinned(),
        &[],
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
                .get("key")
                .expect("restore key")
                .as_str()
        })
        .collect();
    keys.sort_unstable();
    assert_eq!(keys.len(), 2);
    assert_ne!(keys[0], keys[1], "each root owns its key");
    assert!(
        keys.iter()
            .any(|key| key.contains(&velnor_actions_contract::digest_b3(
                format!(
                    "tofu-provider-root-v1\n{}",
                    velnor_actions_tofu::key_for_root("stacks/a")
                )
                .as_bytes()
            ))),
        "{keys:?}"
    );
    assert!(
        keys.iter()
            .any(|key| key.contains(&velnor_actions_contract::digest_b3(
                format!(
                    "tofu-provider-root-v1\n{}",
                    velnor_actions_tofu::key_for_root("stacks/b")
                )
                .as_bytes()
            ))),
        "{keys:?}"
    );
}

#[test]
fn rust_jobs_carry_no_provider_restore() {
    let rust = crate_jobs_tests::group("demo", TaskKind::Clippy, &[]);
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &crate_jobs_tests::discovery(vec![rust]),
        &ToolCatalog::pinned(),
        &[String::new()],
        &[],
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1);
    let names = crate_jobs_tests::names(&found.jobs[0].1);
    assert!(
        !names.contains(&"Restore Tofu providers"),
        "rust jobs restore no providers: {names:?}"
    );
}

#[test]
fn mixed_job_restores_providers_before_obligations() {
    use velnor_actions_tofu::TofuTaskKind;
    let mut rust = crate_jobs_tests::group("demo", TaskKind::Clippy, &[]);
    let tofu = tofu_group("stacks/a", TofuTaskKind::Validate);
    rust.identity.unit_id.clone_from(&tofu.identity.unit_id);
    rust.configuration.clone_from(&tofu.configuration);
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &crate_jobs_tests::discovery(vec![rust, tofu]),
        &ToolCatalog::pinned(),
        &[String::new()],
        &[],
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1, "shared group renders once");
    let names = crate_jobs_tests::names(&found.jobs[0].1);
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
    let err = crate::tofu_cache::tofu_providers_cache_key(
        "mips-unknown-linux",
        "1.13.1",
        "stacks/a",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    )
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
                "stacks/a",
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            )
            .is_err(),
            "{version:?} must fail closed"
        );
    }
    assert!(
        crate::tofu_cache::tofu_providers_cache_key(
            "x86_64-unknown-linux-gnu",
            "1.13.1",
            "stacks/a",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
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
        let err = crate::tofu_cache::tofu_providers_cache_key(
            "x86_64-unknown-linux-gnu",
            "1.13.1",
            root,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
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
        let err = crate::tofu_cache::tofu_providers_cache_key(
            "x86_64-unknown-linux-gnu",
            "1.13.1",
            root,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .expect_err("leading-dash roots fail closed");
        assert!(
            err.to_string().contains("leading_dash_root"),
            "{root:?}: {err}"
        );
    }
}

#[test]
fn provider_key_rejects_runtime_and_unqualified_source_digests() {
    for digest in [
        "",
        "${{hashFiles('.terraform.lock.hcl')}}",
        "b3-012345",
        "ABCDEF",
    ] {
        let error = crate::tofu_cache::tofu_providers_cache_key(
            "x86_64-unknown-linux-gnu",
            "1.13.1",
            "",
            digest,
        )
        .expect_err("source digest must be literal lower hexadecimal");
        assert!(error.to_string().contains("invalid_provider_source_digest"));
    }
}

#[test]
fn provider_key_bounds_long_roots_and_preserves_root_identity() {
    let deep = format!("{}leaf", "nest/".repeat(200));
    let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let key = crate::tofu_cache::tofu_providers_cache_key(
        "x86_64-unknown-linux-gnu",
        "1.13.1",
        &deep,
        digest,
    )
    .expect("long roots use full root hash locator");
    assert!(key.len() <= velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES);
    let first = crate::tofu_cache::tofu_providers_cache_key(
        "x86_64-unknown-linux-gnu",
        "1.13.1",
        "",
        digest,
    )
    .expect("repository root");
    let second = crate::tofu_cache::tofu_providers_cache_key(
        "x86_64-unknown-linux-gnu",
        "1.13.1",
        "root",
        digest,
    )
    .expect("literal root directory");
    assert_ne!(first, second);
}
