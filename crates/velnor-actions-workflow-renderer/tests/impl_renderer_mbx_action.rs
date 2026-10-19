use velnor_actions_contract::{Step, StepKind, WorkflowPolicy};
use velnor_actions_workflow_renderer::steps::{
    CompileDriver, MBX_INSTALL_NAME, MBX_PREFLIGHT_NAME, mbx_steps_for_driver,
};
use velnor_actions_workflow_renderer::toolchain_env::credential_scrub;
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::{
    TEST_MBX_VERSION, TEST_RUST_TOOLCHAIN, fixture_ctx, fixture_ir, job, mbx_tool_env, step_names,
};

const MBX_ACTION: &str = "jdx/mr-boxington-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn steps(
    version: &str,
    rust: &str,
) -> Result<[Step; 3], velnor_actions_workflow_renderer::RenderError> {
    mbx_steps_for_driver(
        MBX_ACTION,
        CompileDriver::Mbx,
        version,
        rust,
        mbx_tool_env(rust),
    )?
    .ok_or_else(|| {
        velnor_actions_workflow_renderer::RenderError::InvalidWorkflow(
            "mbx_steps_missing".to_owned(),
        )
    })
}

fn assert_action_mbx_version_binding(run: &str) {
    assert!(run.contains(&format!("mbx {TEST_MBX_VERSION}")));
    assert!(run.contains("unterminated"));
    assert!(run.contains("mbx_bin=$(command -v mbx)"));
    assert!(run.contains("-- \"$mbx_bin\" --version"));
    assert!(run.contains("-- \"$mbx_bin\" cache dir"));
    assert!(!run.contains("-- mbx --version"));
    assert!(run.contains(&format!(
        "mise --no-config --no-env --no-hooks exec rust@{TEST_RUST_TOOLCHAIN} -- \"$mbx_bin\" --version"
    )));
}

#[test]
fn native_action_is_install_only_with_a_local_in_job_store() {
    let [preflight, action, version_check] =
        steps(TEST_MBX_VERSION, TEST_RUST_TOOLCHAIN).expect("MBX steps");
    assert_eq!(preflight.name, MBX_PREFLIGHT_NAME);
    assert_eq!(
        version_check.name,
        velnor_actions_workflow_renderer::MBX_VERSION_CHECK_NAME
    );
    assert_eq!(version_check.id, None);
    let StepKind::Shell {
        run,
        env: check_env,
    } = version_check.kind
    else {
        panic!("version check must be a shell step");
    };
    assert_action_mbx_version_binding(&run[2]);
    assert!(!run[2].contains(".velnor-source-oids") && !run[2].contains(".velnor-doc"));
    let StepKind::Action { uses, with, env } = action.kind else {
        panic!("install must be an action");
    };
    assert_eq!(action.name, MBX_INSTALL_NAME);
    let mut expected_check_env = env.clone();
    expected_check_env.extend(credential_scrub());
    assert_eq!(
        check_env, expected_check_env,
        "PATH check inherits action-owned settings and adds only the fixed shell credential scrub"
    );
    assert_eq!(uses, MBX_ACTION);
    assert_eq!(
        with,
        std::collections::BTreeMap::from([
            ("backend".to_owned(), "local".to_owned()),
            ("cache-links".to_owned(), "false".to_owned()),
            ("version".to_owned(), TEST_MBX_VERSION.to_owned()),
        ]),
        "no GitHub cache transport, no target payload, no save inputs"
    );
    assert!(!env.contains_key("ACTIONS_CACHE_MODE"));
    assert_eq!(
        env.get("MBX_CACHE_DIR").map(String::as_str),
        Some("${{ runner.temp }}/velnor/mbx"),
        "the action uses the runner-private stable logical store path"
    );
    assert_eq!(env.get("RUSTUP_HOME"), env.get("MISE_RUSTUP_HOME"));
    assert_eq!(env.get("CARGO_HOME"), env.get("MISE_CARGO_HOME"));
}

#[test]
fn action_construction_rejects_floating_refs_and_tool_versions() {
    let mut env = mbx_tool_env(TEST_RUST_TOOLCHAIN);
    for uses in [
        "actions/cache/restore@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "jdx/mr-boxington-action@main",
    ] {
        assert!(
            mbx_steps_for_driver(
                uses,
                CompileDriver::Mbx,
                TEST_MBX_VERSION,
                TEST_RUST_TOOLCHAIN,
                env.clone(),
            )
            .is_err(),
            "invalid action ref {uses}"
        );
    }
    for version in ["latest", "v1.21.1", "1.21", "1.21.1.0", "1.21.x", ""] {
        assert!(
            steps(version, TEST_RUST_TOOLCHAIN).is_err(),
            "invalid MBX version {version:?}"
        );
    }
    for rust in ["stable", "1.98", "1.98.1-nightly", "1.98.1;exit 1"] {
        env = mbx_tool_env(rust);
        assert!(
            mbx_steps_for_driver(MBX_ACTION, CompileDriver::Mbx, TEST_MBX_VERSION, rust, env,)
                .is_err(),
            "invalid Rust toolchain {rust:?}"
        );
    }
}

#[test]
fn cargo_driver_emits_neither_preflight_nor_mbx_action() {
    assert!(
        mbx_steps_for_driver(
            MBX_ACTION,
            CompileDriver::Cargo,
            TEST_MBX_VERSION,
            TEST_RUST_TOOLCHAIN,
            mbx_tool_env(TEST_RUST_TOOLCHAIN),
        )
        .expect("Cargo driver")
        .is_none()
    );
}

#[test]
fn rendered_action_has_rust_homes_and_native_owner_policy() -> Result<(), RenderError> {
    let pair = steps(TEST_MBX_VERSION, TEST_RUST_TOOLCHAIN)?;
    let text = render_workflow_ir(
        &fixture_ir(vec![job("demo", "Demo", Vec::new(), pair.into())]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let names = step_names(&text, "demo");
    let preflight = names
        .iter()
        .position(|name| name == "Verify Rust before MBX action")
        .expect("rendered Rust preflight");
    let action = names
        .iter()
        .position(|name| name == "Install MBX")
        .expect("rendered MBX action");
    assert!(preflight < action, "rendered order: {names:?}");
    let rustup_home = "$".to_owned() + "{{ runner.temp }}/velnor/rustup";
    let cargo_home = "$".to_owned() + "{{ runner.temp }}/velnor/cargo";
    for key in ["MISE_RUSTUP_HOME", "RUSTUP_HOME"] {
        assert!(text.contains(&format!("{key}: {rustup_home}")), "{key}");
    }
    for key in ["MISE_CARGO_HOME", "CARGO_HOME"] {
        assert!(text.contains(&format!("{key}: {cargo_home}")), "{key}");
    }
    assert!(
        text.contains("GITHUB_PATH"),
        "rendered preflight exports PATH"
    );
    assert!(
        text.contains("rust@1.98.1"),
        "rendered preflight probes the catalog toolchain"
    );
    assert!(!text.contains("mr-boxington@"), "Mise does not install MBX");
    assert!(
        !text.contains("$RUNNER_TEMP/velnor/mbx-preflight"),
        "the retired caller-owned fixed scratch leaf is absent"
    );
    assert!(text.contains("MBX_SHARE_OUT_DIR: \"0\""));
    assert!(text.contains("MBX_CC: \"0\""));
    assert!(text.contains("MBX_BUILD_SCRIPT_EXECUTION: \"0\""));
    assert!(text.contains("MBX_CACHE_DIR: ${{ runner.temp }}/velnor/mbx"));
    assert!(text.contains("MBX_GC_AUTO: \"0\""));
    assert!(text.contains("MBX_TARGET_VIEWS: \"0\""));
    assert!(!text.contains("Clean MBX workspace outputs"));
    assert!(!text.contains("github-cache-mode"));
    assert!(!text.contains(".velnor-doc") && !text.contains(".velnor-source-oids"));
    Ok(())
}
