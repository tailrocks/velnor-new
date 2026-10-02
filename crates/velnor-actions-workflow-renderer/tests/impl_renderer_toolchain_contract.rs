//! Validated task-step env contract cases (P07 rendered side).
use std::collections::BTreeMap;

use velnor_actions_workflow_renderer::toolchain_env::{
    CREDENTIAL_UNSET_VARS, STEP_CREDENTIAL_DENYLIST, STEP_ENDPOINT_DENYLIST,
    STEP_ISOLATION_DENYLIST, STEP_TF_ALLOWLIST, STEP_TF_DENYLIST_PREFIXES, TOOLCHAIN_HOME_KEYS,
    checked_project_task_env, checked_task_env, credential_scrub, credential_unset_prelude,
    is_denied_credential_key, is_denied_endpoint_key, is_denied_tf_key, reject_denied_step_keys,
    reject_denied_tf_keys, reject_privileged_task_keys, with_credential_scrub, with_env_unset_argv,
    with_toolchain_homes,
};

#[test]
fn denylist_names_exact_credential_set() {
    assert_eq!(
        STEP_CREDENTIAL_DENYLIST,
        [
            "MISE_GITHUB_TOKEN",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "ACTIONS_RUNTIME_TOKEN",
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            "ACTIONS_ID_TOKEN_REQUEST_URL",
            "CARGO_REGISTRY_TOKEN",
            "NPM_TOKEN",
            "NODE_AUTH_TOKEN",
        ]
    );
    for denied in STEP_CREDENTIAL_DENYLIST {
        assert!(
            !TOOLCHAIN_HOME_KEYS.contains(&denied),
            "{denied} must not collide with the triple"
        );
    }
}

#[test]
fn denied_keys_rejected_anywhere_in_map() {
    assert!(reject_denied_step_keys(&BTreeMap::new()).is_ok());
    let clean = BTreeMap::from([
        ("MISE_RUSTUP_HOME".to_owned(), "/r".to_owned()),
        ("VELNOR_TASK_RUN".to_owned(), "mise run x".to_owned()),
    ]);
    assert!(reject_denied_step_keys(&clean).is_ok());
    for denied in STEP_CREDENTIAL_DENYLIST {
        let mut dirty = clean.clone();
        dirty.insert(denied.to_owned(), "sentinel".to_owned());
        let err = reject_denied_step_keys(&dirty).expect_err("denied key must fail");
        assert!(
            err.to_string()
                .contains(&format!("credential_step_env:{denied}")),
            "got {err}"
        );
    }
}

#[test]
fn credential_scrub_blanks_all_denied_keys() {
    let scrub = credential_scrub();
    assert_eq!(
        scrub.len(),
        STEP_CREDENTIAL_DENYLIST.len() + STEP_ENDPOINT_DENYLIST.len()
    );
    for denied in STEP_CREDENTIAL_DENYLIST
        .into_iter()
        .chain(STEP_ENDPOINT_DENYLIST)
    {
        assert_eq!(
            scrub.get(denied).map(String::as_str),
            Some(""),
            "{denied} must scrub empty"
        );
    }
    let base = BTreeMap::from([("MISE_NO_CONFIG".to_owned(), "1".to_owned())]);
    let scrubbed = with_credential_scrub(&base);
    assert_eq!(
        scrubbed.get("MISE_NO_CONFIG").map(String::as_str),
        Some("1")
    );
    for denied in STEP_CREDENTIAL_DENYLIST
        .into_iter()
        .chain(STEP_ENDPOINT_DENYLIST)
    {
        assert_eq!(
            scrubbed.get(denied).map(String::as_str),
            Some(""),
            "{denied} must scrub empty"
        );
    }
}

#[test]
fn endpoint_denylist_names_exact_selector_set() {
    assert_eq!(STEP_ENDPOINT_DENYLIST, ["GH_HOST", "GH_CONFIG_DIR"]);
    for denied in STEP_ENDPOINT_DENYLIST {
        assert!(is_denied_endpoint_key(denied), "{denied} must match");
        assert!(
            !TOOLCHAIN_HOME_KEYS.contains(&denied),
            "{denied} must not collide with the triple"
        );
        let mut dirty = BTreeMap::new();
        dirty.insert(denied.to_owned(), "sentinel".to_owned());
        let err = reject_denied_step_keys(&dirty).expect_err("endpoint key must fail");
        assert!(
            err.to_string()
                .contains(&format!("endpoint_step_env:{denied}")),
            "got {err}"
        );
    }
}

#[test]
fn checked_task_env_merges_triple_over_validated_base() {
    let base = BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        (
            "VELNOR_TASK_ID".to_owned(),
            "${{ matrix.task_id }}".to_owned(),
        ),
    ]);
    let merged = checked_task_env(&base, "/r/rustup", "/r/cargo", "1.98.1").expect("checked env");
    assert_eq!(
        merged.get("MISE_RUSTUP_HOME").map(String::as_str),
        Some("/r/rustup")
    );
    assert_eq!(
        merged.get("RUSTUP_TOOLCHAIN").map(String::as_str),
        Some("1.98.1")
    );
    assert_eq!(
        merged.get("VELNOR_TASK_ID").map(String::as_str),
        Some("${{ matrix.task_id }}")
    );
    assert_eq!(merged.get("MISE_NO_CONFIG").map(String::as_str), Some("1"));
}

#[test]
fn checked_task_env_rejects_blank_triple_inputs() {
    let base = BTreeMap::new();
    for (rustup, cargo, toolchain) in [
        ("", "/r/cargo", "1.98.1"),
        ("/r/rustup", "", "1.98.1"),
        ("/r/rustup", "/r/cargo", ""),
    ] {
        let err = checked_task_env(&base, rustup, cargo, toolchain).expect_err("blank must fail");
        assert!(
            err.to_string().contains("missing_toolchain_home"),
            "got {err}"
        );
    }
}

#[test]
fn checked_task_env_rejects_denied_base_keys() {
    for denied in STEP_CREDENTIAL_DENYLIST {
        let base = BTreeMap::from([(denied.to_owned(), "sentinel".to_owned())]);
        let err =
            checked_task_env(&base, "/r/rustup", "/r/cargo", "1.98.1").expect_err("denied in base");
        assert!(
            err.to_string()
                .contains(&format!("credential_step_env:{denied}")),
            "got {err}"
        );
    }
}

#[test]
fn legacy_merge_stays_unvalidated_for_compat() {
    // `with_toolchain_homes` keeps its infallible shape for existing
    // callers; only `checked_task_env` enforces the contract.
    let base = BTreeMap::from([("MISE_GITHUB_TOKEN".to_owned(), "x".to_owned())]);
    let merged = with_toolchain_homes(&base, "/r/rustup", "/r/cargo", "1.98.1");
    assert!(merged.contains_key("MISE_GITHUB_TOKEN"));
    assert!(reject_denied_step_keys(&merged).is_err());
}

#[test]
fn isolation_denylist_names_exact_privileged_set() {
    assert_eq!(
        STEP_ISOLATION_DENYLIST,
        [
            "MISE_NO_CONFIG",
            "MISE_NO_ENV",
            "MISE_NO_HOOKS",
            "MISE_LOCKFILE",
            "MISE_AUTO_INSTALL",
            "MISE_EXEC_AUTO_INSTALL",
        ]
    );
    for denied in STEP_ISOLATION_DENYLIST {
        assert!(
            !TOOLCHAIN_HOME_KEYS.contains(&denied),
            "{denied} must not collide with the triple"
        );
        assert!(
            !STEP_CREDENTIAL_DENYLIST.contains(&denied),
            "{denied} must not duplicate the credential set"
        );
    }
}

#[test]
fn project_task_env_rejects_hook_escape_keys() {
    assert!(reject_privileged_task_keys(&BTreeMap::new()).is_ok());
    for hostile in STEP_ISOLATION_DENYLIST {
        for value in ["0", "false", "1"] {
            let base = BTreeMap::from([(hostile.to_owned(), value.to_owned())]);
            let err = reject_privileged_task_keys(&base).expect_err("privileged must fail");
            assert!(
                err.to_string()
                    .contains(&format!("privileged_task_env:{hostile}")),
                "got {err}"
            );
            let err = checked_project_task_env(&base, "/r/rustup", "/r/cargo", "1.98.1")
                .expect_err("project task must fail");
            assert!(
                err.to_string()
                    .contains(&format!("privileged_task_env:{hostile}")),
                "got {err}"
            );
        }
    }
    for denied in STEP_CREDENTIAL_DENYLIST {
        let base = BTreeMap::from([(denied.to_owned(), "sentinel".to_owned())]);
        assert!(
            checked_project_task_env(&base, "/r/rustup", "/r/cargo", "1.98.1").is_err(),
            "project task smuggling {denied} must fail"
        );
    }
}

#[test]
fn credential_pattern_catches_registry_and_enterprise_variants() {
    for denied in [
        "CARGO_REGISTRIES_ACME_TOKEN",
        "CARGO_REGISTRIES_ENTERPRISE",
        "NPM_TOKEN",
        "NODE_AUTH_TOKEN",
        "MY_REGISTRY_TOKEN",
        "GITHUB_TOKEN",
    ] {
        assert!(is_denied_credential_key(denied), "{denied} must match");
        let base = BTreeMap::from([(denied.to_owned(), "sentinel".to_owned())]);
        assert!(
            reject_denied_step_keys(&base).is_err(),
            "{denied} must fail in a step map"
        );
    }
    for clean in [
        "MISE_RUSTUP_HOME",
        "VELNOR_TASK_RUN",
        "TOKEN_COUNT",
        "MY_TOKENS",
        "CARGO_REGISTRIES",
    ] {
        assert!(!is_denied_credential_key(clean), "{clean} must pass");
    }
    assert!(reject_denied_step_keys(&BTreeMap::new()).is_ok());
}

#[test]
fn unset_wrappers_cover_every_unshadowable_var() {
    assert_eq!(
        CREDENTIAL_UNSET_VARS,
        [
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            "ACTIONS_ID_TOKEN_REQUEST_URL",
            "ACTIONS_RUNTIME_TOKEN",
            "GITHUB_TOKEN",
            "MISE_GITHUB_TOKEN",
            "GH_TOKEN",
            "GH_HOST",
            "GH_CONFIG_DIR",
        ]
    );
    let prelude = credential_unset_prelude();
    assert!(prelude.starts_with("unset "), "prelude must unset");
    for var in CREDENTIAL_UNSET_VARS {
        assert!(prelude.contains(var), "{var} must unset in scripts");
    }
    let argv = with_env_unset_argv(&["mise".to_owned(), "run".to_owned(), "x".to_owned()]);
    assert_eq!(argv.first().map(String::as_str), Some("env"));
    for var in CREDENTIAL_UNSET_VARS {
        let at = argv.iter().position(|arg| arg == var).expect(var);
        assert_eq!(argv[at - 1], "-u", "{var} must pair with -u");
    }
    assert_eq!(&argv[argv.len() - 3..], ["mise", "run", "x"]);
}

#[test]
fn tf_family_keys_denied_except_automation_pair() {
    assert_eq!(STEP_TF_DENYLIST_PREFIXES, ["TF_", "CHECKPOINT_"]);
    assert_eq!(STEP_TF_ALLOWLIST, ["TF_IN_AUTOMATION", "TF_INPUT"]);
    for denied in [
        "TF_VAR_secret",
        "TF_CLI_ARGS_plan",
        "TF_TOKEN_app",
        "TF_DATA_DIR",
        "TF_CLI_CONFIG_FILE",
        "TF_WORKSPACE",
        "TF_LOG",
        "TF_LOG_PATH",
        "TF_REGISTRY_CLIENT_TIMEOUT",
        "CHECKPOINT_DISABLE",
    ] {
        assert!(is_denied_tf_key(denied), "{denied} must match");
        let base = BTreeMap::from([(denied.to_owned(), "sentinel".to_owned())]);
        assert!(
            reject_denied_tf_keys(&base).is_err(),
            "{denied} must fail in a project map"
        );
    }
    for clean in [
        "TF_IN_AUTOMATION",
        "TF_INPUT",
        "MISE_RUSTUP_HOME",
        "VELNOR_TASK_RUN",
        "PATH",
    ] {
        assert!(!is_denied_tf_key(clean), "{clean} must pass");
    }
    assert!(reject_denied_tf_keys(&BTreeMap::new()).is_ok());
}

#[test]
fn project_task_env_rejects_tf_keys() {
    for hostile in ["TF_VAR_evil", "TF_DATA_DIR", "CHECKPOINT_DISABLE"] {
        let base = BTreeMap::from([(hostile.to_owned(), "evil".to_owned())]);
        let err = checked_project_task_env(&base, "/r/rustup", "/r/cargo", "1.98.1")
            .expect_err("hostile TF key must fail");
        assert!(
            err.to_string().contains(&format!("tf_task_env:{hostile}")),
            "got {err}"
        );
    }
    let base = BTreeMap::from([
        ("TF_IN_AUTOMATION".to_owned(), "1".to_owned()),
        ("TF_INPUT".to_owned(), "0".to_owned()),
    ]);
    let merged = checked_project_task_env(&base, "/r/rustup", "/r/cargo", "1.98.1")
        .expect("automation pair stays allowed");
    assert_eq!(
        merged.get("TF_IN_AUTOMATION").map(String::as_str),
        Some("1")
    );
}

#[test]
fn project_task_env_merges_clean_declarations() {
    let base = BTreeMap::from([("VELNOR_TASK_RUN".to_owned(), "lint".to_owned())]);
    let merged = checked_project_task_env(&base, "/r/rustup", "/r/cargo", "1.98.1")
        .expect("clean project env");
    assert_eq!(
        merged.get("MISE_RUSTUP_HOME").map(String::as_str),
        Some("/r/rustup")
    );
    assert_eq!(
        merged.get("VELNOR_TASK_RUN").map(String::as_str),
        Some("lint")
    );
    let err =
        checked_project_task_env(&base, "", "/r/cargo", "1.98.1").expect_err("blank must fail");
    assert!(
        err.to_string().contains("missing_toolchain_home"),
        "got {err}"
    );
}
