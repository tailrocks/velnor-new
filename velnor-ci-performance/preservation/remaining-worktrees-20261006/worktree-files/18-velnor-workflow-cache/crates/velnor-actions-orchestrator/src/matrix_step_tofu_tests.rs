//! Tofu obligation-step contract tests.
//!
//! Declared via `#[path]` from `matrix_step.rs` under `cfg(test)`.

use super::*;

#[test]
fn tofu_step_env_rejects_triple_and_reserved_keys() {
    use velnor_actions_mise::{
        MISE_CARGO_HOME_ENV, MISE_DATA_DIR_ENV, MISE_RUSTUP_HOME_ENV, RUSTUP_TOOLCHAIN_ENV,
    };
    let catalog = ToolCatalog::pinned();
    let env = task_step_env(&catalog, &BTreeMap::new(), false).expect("tofu env");
    assert_eq!(
        env.get(MISE_DATA_DIR_ENV).map(String::as_str),
        Some(velnor_actions_mise::runtime_paths::MISE_DATA_DIR_EXPR),
        "tofu tasks use the explicit workspace Mise data dir, independent of ambient defaults"
    );
    for key in [
        "MISE_NO_CONFIG",
        "MISE_NO_ENV",
        "MISE_NO_HOOKS",
        "MISE_LOCKFILE",
        "MISE_AUTO_INSTALL",
        "MISE_EXEC_AUTO_INSTALL",
    ] {
        assert!(
            env.contains_key(key),
            "tofu env keeps the isolation overlay: {env:?}"
        );
    }
    for key in [
        MISE_DATA_DIR_ENV,
        MISE_RUSTUP_HOME_ENV,
        MISE_CARGO_HOME_ENV,
        "RUSTUP_HOME",
        "CARGO_HOME",
        RUSTUP_TOOLCHAIN_ENV,
    ] {
        if key != MISE_DATA_DIR_ENV {
            assert!(
                !env.contains_key(key),
                "tofu env carries no Rust home {key}"
            );
        }
        let extra = BTreeMap::from([(key.to_owned(), "/x".to_owned())]);
        let err = task_step_env(&catalog, &extra, false).expect_err("tool home extra");
        assert!(
            err.to_string().contains("reserved_step_env"),
            "triple extras fail closed: {err}"
        );
    }
    let extra = BTreeMap::from([("TF_VAR_secret".to_owned(), "x".to_owned())]);
    let err = task_step_env(&catalog, &extra, false).expect_err("reserved extra");
    assert!(
        err.to_string().contains("reserved_step_env"),
        "reserved extras fail closed: {err}"
    );
    let rust = task_step_env(&catalog, &BTreeMap::new(), true).expect("rust env");
    assert_eq!(
        rust.get(MISE_DATA_DIR_ENV).map(String::as_str),
        Some(velnor_actions_mise::runtime_paths::MISE_DATA_DIR_EXPR)
    );
    for key in [
        MISE_RUSTUP_HOME_ENV,
        MISE_CARGO_HOME_ENV,
        RUSTUP_TOOLCHAIN_ENV,
    ] {
        assert!(
            rust.get(key).is_some_and(|value| !value.is_empty()),
            "rust env keeps the triple: {rust:?}"
        );
    }
}

/// Tofu obligation fixture for step construction.
fn tofu_obligation() -> CrateObligation {
    CrateObligation {
        task_id: "stack/tofu/root/init/default".to_owned(),
        kind: "init".to_owned(),
        step_name: "Init for validate".to_owned(),
        gated_by: Vec::new(),
        matrix_key: "m-0123456789abcdef".to_owned(),
        task_digest: format!("b3-{}", "a".repeat(64)),
        run: vec!["true".to_owned()],
    }
}

#[test]
fn tofu_step_names_render_through_tofu_table() {
    assert_eq!(
        step_name_for("init", "stack/tofu/root/init/default"),
        "Init for validate"
    );
    assert_eq!(
        step_name_for("validate", "stack/tofu/root/validate/default"),
        "Validate"
    );
    assert_eq!(
        step_name_for("fmt", "stack/tofu/root/fmt/default"),
        FORMAT_STEP_NAME
    );
    assert_eq!(
        step_name_for("clippy", "stack/rust/demo/clippy/default"),
        "Clippy"
    );
}

#[test]
fn pure_tofu_prepare_init_and_validate_share_explicit_mise_data_dir() {
    let catalog = ToolCatalog::pinned();
    let prepare =
        prepare_crate_tools_step(&catalog, false, false, false, true, false).expect("prepare");
    let velnor_actions_contract::StepKind::Shell {
        env: prepare_env, ..
    } = &prepare.kind
    else {
        panic!("prepare must be a shell step");
    };
    let expected = velnor_actions_mise::runtime_paths::MISE_DATA_DIR_EXPR;
    assert_eq!(
        prepare_env
            .get(velnor_actions_mise::MISE_DATA_DIR_ENV)
            .map(String::as_str),
        Some(expected)
    );

    let mut validate = tofu_obligation();
    validate.task_id = "stack/tofu/root/validate/default".to_owned();
    validate.kind = "validate".to_owned();
    validate.step_name = "Validate".to_owned();
    for obligation in [tofu_obligation(), validate] {
        let step = obligation_step(&obligation, &catalog, &[], None).expect("tofu step");
        let velnor_actions_contract::StepKind::Shell { env, .. } = &step.kind else {
            panic!("tofu obligation must be a shell step");
        };
        assert_eq!(
            env.get(velnor_actions_mise::MISE_DATA_DIR_ENV)
                .map(String::as_str),
            Some(expected),
            "{} uses the same explicit Mise data dir",
            obligation.step_name
        );
        for key in [
            velnor_actions_mise::MISE_RUSTUP_HOME_ENV,
            velnor_actions_mise::MISE_CARGO_HOME_ENV,
            "RUSTUP_HOME",
            "CARGO_HOME",
            velnor_actions_mise::RUSTUP_TOOLCHAIN_ENV,
        ] {
            assert!(
                !env.contains_key(key),
                "{} carries no Rust home {key}",
                obligation.step_name
            );
        }
    }
}

#[test]
fn tofu_obligation_step_carries_tofu_matrix_id_and_no_doc_env() {
    use velnor_actions_rust::RUSTDOCFLAGS_ENV;
    let step =
        obligation_step(&tofu_obligation(), &ToolCatalog::pinned(), &[], None).expect("step");
    let velnor_actions_contract::StepKind::Shell { env, .. } = &step.kind else {
        panic!("obligation must be a shell step");
    };
    assert_eq!(
        env.get(OBLIGATION_MATRIX_ID_ENV).map(String::as_str),
        Some("stack:tofu|task:stack/tofu/root/init/default")
    );
    assert!(
        !env.contains_key(RUSTDOCFLAGS_ENV),
        "tofu must not carry doc env"
    );
}

#[test]
fn tofu_obligation_step_carries_isolated_cache_dir() {
    let step =
        obligation_step(&tofu_obligation(), &ToolCatalog::pinned(), &[], None).expect("step");
    let velnor_actions_contract::StepKind::Shell { env, .. } = &step.kind else {
        panic!("obligation must be a shell step");
    };
    assert_eq!(
        env.get(velnor_actions_mise::MISE_DATA_DIR_ENV)
            .map(String::as_str),
        Some(velnor_actions_mise::runtime_paths::MISE_DATA_DIR_EXPR)
    );
    for key in [
        velnor_actions_mise::MISE_RUSTUP_HOME_ENV,
        velnor_actions_mise::MISE_CARGO_HOME_ENV,
        "RUSTUP_HOME",
        "CARGO_HOME",
        velnor_actions_mise::RUSTUP_TOOLCHAIN_ENV,
    ] {
        assert!(
            !env.contains_key(key),
            "tofu step carries no Rust home {key}"
        );
    }
    let data = env
        .get(velnor_actions_tofu::TF_DATA_DIR_ENV)
        .expect("data dir");
    let cache = env
        .get(velnor_actions_tofu::TF_PLUGIN_CACHE_DIR_ENV)
        .expect("cache dir");
    assert!(
        data.starts_with("${{ runner.temp }}/velnor/tofu-data/root-"),
        "{data}"
    );
    assert!(
        cache.starts_with("${{ runner.temp }}/velnor/tofu-cache/root-"),
        "{cache}"
    );
    let data_slug = data.rsplit('/').next().expect("slug");
    let cache_slug = cache.rsplit('/').next().expect("slug");
    assert_eq!(data_slug, cache_slug, "one slug, two bases");
}

#[test]
fn stackless_obligation_task_ids_keep_malformed_vocabulary() {
    let mut bad = tofu_obligation();
    bad.task_id = "bogus".to_owned();
    let err = obligation_step(&bad, &ToolCatalog::pinned(), &[], None).expect_err("must fail");
    assert!(err.to_string().contains("malformed_task_id"), "{err}");
}
