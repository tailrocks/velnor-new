//! Tofu proposal + task-kind cases (T12).
use velnor_actions_contract::ResourceClass;
use velnor_actions_tofu::argv::tofu_payload_argv;
use velnor_actions_tofu::kinds::TofuTaskKind;
use velnor_actions_tofu::propose::{
    KIND_DISPLAY_WORDS, TOFU_DRIVER, TOFU_RUNNER, TofuTaskGroup, display_for_root, is_init_kind,
    is_validate_kind, key_for_root, payload_env_for_kind, propose_task, resource_class_for_kind,
    root_for_key, step_base_name, task_id_for_root, task_kind_rank,
};

/// Contract-exact variant keeps the `init` wire spelling.
#[test]
fn init_for_validate_keeps_init_spelling() {
    assert_eq!(TofuTaskKind::InitForValidate.as_str(), "init");
    assert_eq!(
        TofuTaskKind::parse("init"),
        Ok(TofuTaskKind::InitForValidate)
    );
}

/// Only the three wire spellings parse; long forms fail closed.
#[test]
fn unknown_kind_spellings_fail_closed() {
    for spelling in ["InitForValidate", "init-for-validate", "INIT", "", "plan"] {
        let err = TofuTaskKind::parse(spelling).expect_err("must fail closed");
        assert!(
            err.to_string()
                .contains(&format!("unknown_kind:{spelling}")),
            "{spelling}: {err}"
        );
    }
}

/// Exact UTF-8 roots remain distinct through the identity namespace.
#[test]
fn key_mapping_round_trips_roots() {
    let roots = [
        "",
        "root",
        "dir-",
        "a-b",
        "a_b",
        "a/b",
        "infra/日本語",
        "é",
        "e\u{301}",
    ];
    let keys: std::collections::BTreeSet<String> =
        roots.iter().map(|root| key_for_root(root)).collect();
    assert_eq!(keys.len(), roots.len());
    for root in roots {
        assert_eq!(root_for_key(&key_for_root(root)).expect("decode"), root);
        for kind in [
            TofuTaskKind::Fmt,
            TofuTaskKind::InitForValidate,
            TofuTaskKind::Validate,
        ] {
            let task = propose_task(&group(root, kind)).expect("proposal");
            task.validate().expect("valid task");
            assert_eq!(task.identity.project_root, display_for_root(root));
        }
    }
    assert_eq!(display_for_root(""), ".");
}

#[test]
fn malformed_root_keys_and_noncanonical_paths_fail_closed() {
    for key in [
        "root",
        "stacks/a",
        "dir-0",
        "dir-AA",
        "dir-ff",
        "dir-2e",
        "dir-2f61",
        "dir-612f2f62",
        "dir-612f2e2e",
        "dir-00",
    ] {
        assert!(root_for_key(key).is_err(), "{key}");
    }
    for root in [".", "..", "/a", "a//b", "a/../b", "a\\b", "a\n"] {
        assert!(
            task_id_for_root(root, TofuTaskKind::Fmt, "default").is_err(),
            "{root:?}"
        );
    }
}

/// Task IDs flow through the existing stack grammar per root/kind.
#[test]
fn task_ids_flow_through_stack_grammar() {
    assert_eq!(
        task_id_for_root("", TofuTaskKind::Fmt, "default").expect("root fmt"),
        "stack/tofu/dir-/fmt/default"
    );
    assert_eq!(
        task_id_for_root("", TofuTaskKind::InitForValidate, "default").expect("root init"),
        "stack/tofu/dir-/init/default"
    );
    assert_eq!(
        task_id_for_root("stacks/a", TofuTaskKind::Validate, "default").expect("nested validate"),
        "stack/tofu/dir-737461636b732f61/validate/default"
    );
    assert!(task_id_for_root("", TofuTaskKind::Fmt, "/abs").is_err());
}

/// One group per root/kind for proposal tests.
fn group(root: &str, kind: TofuTaskKind) -> TofuTaskGroup {
    TofuTaskGroup {
        root: root.to_owned(),
        kind,
        configuration: "default".to_owned(),
        no_targets: false,
    }
}

/// Validate depends on same-root Init; Fmt and Init are independent.
#[test]
fn validate_depends_on_same_root_init() {
    let validate = propose_task(&group("stacks/a", TofuTaskKind::Validate)).expect("proposes");
    assert_eq!(
        validate.depends_on,
        vec!["stack/tofu/dir-737461636b732f61/init/default".to_owned()]
    );
    assert!(validate.gated_by.is_empty());
    for kind in [TofuTaskKind::Fmt, TofuTaskKind::InitForValidate] {
        let task = propose_task(&group("stacks/a", kind)).expect("proposes");
        assert!(task.depends_on.is_empty(), "{} independent", kind.as_str());
        assert!(task.gated_by.is_empty());
    }
}

/// Independent mutation of a root authority or root edge fails admission.
#[test]
fn proposal_root_authorities_must_all_agree() {
    for root in ["", "root", "infra/日本語"] {
        for kind in [
            TofuTaskKind::Fmt,
            TofuTaskKind::InitForValidate,
            TofuTaskKind::Validate,
        ] {
            let task = propose_task(&group(root, kind)).expect("proposal");
            assert_eq!(
                velnor_actions_tofu::normalized_root_for_proposal(&task).expect("admit"),
                root
            );
            let mut mutations = Vec::new();
            let mut changed = task.clone();
            changed.identity.project_root = "different".to_owned();
            mutations.push(changed);
            let mut changed = task.clone();
            changed.identity.unit_key = key_for_root("different");
            mutations.push(changed);
            let mut changed = task.clone();
            changed.identity.unit_id = key_for_root("different");
            mutations.push(changed);
            let mut changed = task.clone();
            changed.identity.unit_path = "different".to_owned();
            mutations.push(changed);
            let mut changed = task.clone();
            changed.component_id = "different".to_owned();
            mutations.push(changed);
            let mut changed = task.clone();
            changed.task_id = task_id_for_root("different", kind, "default").expect("ID");
            mutations.push(changed);
            let mut changed = task.clone();
            changed.payload = tofu_payload_argv(kind, "different").expect("argv");
            mutations.push(changed);
            let mut changed = task.clone();
            changed.depends_on = vec!["stack/tofu/dir-646966666572656e74/init/default".to_owned()];
            mutations.push(changed);
            let mut changed = task.clone();
            changed.reads = vec!["different".to_owned()];
            mutations.push(changed);
            for changed in mutations {
                assert!(velnor_actions_tofu::normalized_root_for_proposal(&changed).is_err());
            }
        }
    }
}

/// Proposals populate IDs, identity, resources, and the stub payload.
#[test]
fn proposals_populate_identity_and_validate() {
    for root in ["", "stacks/a"] {
        for kind in [
            TofuTaskKind::Fmt,
            TofuTaskKind::InitForValidate,
            TofuTaskKind::Validate,
        ] {
            let task = propose_task(&group(root, kind)).expect("proposes");
            let key = key_for_root(root);
            assert_eq!(task.stack_id, "tofu");
            assert_eq!(task.task_kind, kind.as_str());
            assert_eq!(task.identity.unit_id, key);
            assert_eq!(task.identity.unit_key, key);
            assert_eq!(task.identity.unit_path, display_for_root(root));
            assert_eq!(task.identity.compile_driver, TOFU_DRIVER);
            assert_eq!(task.identity.test_runner, TOFU_RUNNER);
            assert_eq!(task.identity.target, "host");
            assert_eq!(
                task.identity
                    .environment
                    .get("TF_IN_AUTOMATION")
                    .map(String::as_str),
                Some("1"),
                "{root} {} env",
                kind.as_str()
            );
            assert_eq!(
                task.identity
                    .environment
                    .get("TF_INPUT")
                    .map(String::as_str),
                Some("0"),
                "{root} {} env",
                kind.as_str()
            );
            assert!(!task.identity.undeclared_reads);
            assert_eq!(
                task.payload,
                tofu_payload_argv(kind, root).expect("direct payload"),
                "{root} {} payload",
                kind.as_str()
            );
            assert_eq!(task.display_name, display_for_root(root));
            assert_eq!(task.runner_profile, "default");
            assert!(!task.uses_clock && !task.uses_random && !task.no_targets);
            assert_eq!(task.reads, vec![display_for_root(root)]);
            assert!(task.writes.is_empty() && task.outputs.is_empty());
            assert!(task.validate().is_ok(), "{root} {} valid", kind.as_str());
        }
    }
}

/// Init is network-class; fmt and validate are lightweight and offline.
#[test]
fn resource_classes_pin_init_network() {
    let init = propose_task(&group("", TofuTaskKind::InitForValidate)).expect("proposes");
    assert_eq!(init.resource.class, ResourceClass::Network);
    assert!(init.resource.needs_network);
    for kind in [TofuTaskKind::Fmt, TofuTaskKind::Validate] {
        let task = propose_task(&group("", kind)).expect("proposes");
        assert_eq!(task.resource.class, ResourceClass::Lightweight);
        assert!(!task.resource.needs_network);
    }
    assert!(
        !propose_task(&group("", TofuTaskKind::Fmt))
            .expect("proposes")
            .cache_policy
            .allow_compilation_reuse
    );
}

/// Proposals carry fixed per-kind argv, never a placeholder marker.
#[test]
fn payloads_are_fixed_shapes_not_placeholders() {
    for root in ["", "stacks/a"] {
        for kind in [
            TofuTaskKind::Fmt,
            TofuTaskKind::InitForValidate,
            TofuTaskKind::Validate,
        ] {
            let task = propose_task(&group(root, kind)).expect("proposes");
            assert!(
                !task
                    .payload
                    .iter()
                    .any(|arg| arg.to_string_lossy().contains("pending_t13")),
                "{root} {} unmarked",
                kind.as_str()
            );
            assert_eq!(
                task.payload
                    .first()
                    .map(|arg| arg.to_string_lossy().into_owned()),
                Some(if root.is_empty() {
                    kind.as_str().to_owned()
                } else {
                    "-chdir".to_owned()
                }),
                "{root} {} head",
                kind.as_str()
            );
        }
    }
}

/// Empty fmt scopes carry `no_targets` without breaking validation.
#[test]
fn no_targets_preserved_for_empty_fmt_scope() {
    let mut empty = group("", TofuTaskKind::Fmt);
    empty.no_targets = true;
    let task = propose_task(&empty).expect("proposes");
    assert!(task.no_targets);
    assert!(task.validate().is_ok());
}

/// Dispatch helpers pin every tofu kind spelling and rank.
#[test]
fn dispatch_helpers_pin_spellings() {
    assert_eq!(task_kind_rank("fmt"), 0);
    assert_eq!(task_kind_rank("init"), 1);
    assert_eq!(task_kind_rank("validate"), 2);
    assert_eq!(task_kind_rank("bogus"), u32::MAX);
    assert_eq!(step_base_name("fmt", "Format"), "Format");
    assert_eq!(step_base_name("init", "Format"), "Init for validate");
    assert_eq!(step_base_name("validate", "Format"), "Validate");
    assert_eq!(step_base_name("bogus", "Format"), "bogus");
    assert_eq!(KIND_DISPLAY_WORDS.len(), 3);
    assert!(KIND_DISPLAY_WORDS.contains(&("init", "init for validate")));
    assert!(is_init_kind("init"));
    assert!(!is_init_kind("validate"));
    assert!(is_validate_kind("validate"));
    assert!(!is_validate_kind("init"));
    assert!(!is_init_kind("fmt") && !is_validate_kind("fmt"));
    assert_eq!(payload_env_for_kind("fmt").len(), 2);
    assert_eq!(payload_env_for_kind("init").len(), 2);
    assert_eq!(payload_env_for_kind("validate").len(), 2);
    assert!(payload_env_for_kind("bogus").is_empty());
    assert_eq!(
        resource_class_for_kind(TofuTaskKind::InitForValidate),
        ResourceClass::Network
    );
    assert_eq!(
        resource_class_for_kind(TofuTaskKind::Fmt),
        ResourceClass::Lightweight
    );
}

/// Reuse flags match the enforced gate: init/validate proposals say
/// false (task-result reuse stays disabled), fmt stays true.
#[test]
fn reuse_flags_match_the_enforced_gate() -> Result<(), velnor_actions_contract::ContractError> {
    for (kind, reuse) in [
        (TofuTaskKind::InitForValidate, false),
        (TofuTaskKind::Validate, false),
        (TofuTaskKind::Fmt, true),
    ] {
        let task = propose_task(&group("stacks/a", kind))?;
        assert_eq!(
            task.cache_policy.allow_task_reuse,
            reuse,
            "{} must pin its flag",
            kind.as_str()
        );
    }
    Ok(())
}
