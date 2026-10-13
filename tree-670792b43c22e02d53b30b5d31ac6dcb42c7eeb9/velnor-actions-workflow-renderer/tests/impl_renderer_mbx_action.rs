use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_contract_workflow::{Step, StepKind};
use velnor_actions_workflow_cache::cache_steps::{
    CompileDriver, MBX_CACHE_MODE_ENV, MBX_PREFLIGHT_NAME, mbx_steps_for_driver,
};
use velnor_actions_workflow_renderer::render_workflow_ir;
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_steps::toolchain_env::credential_scrub;

use super::impl_renderer_fixtures::{
    TEST_MBX_VERSION, TEST_RUST_TOOLCHAIN, fixture_ctx, fixture_ir, job, mbx_tool_env, step_names,
};

const MBX_ACTION: &str = "jdx/mr-boxington-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn steps(
    version: &str,
    rust: &str,
) -> Result<[Step; 3], velnor_actions_workflow_steps::RenderError> {
    mbx_steps_for_driver(
        MBX_ACTION,
        CompileDriver::Mbx,
        version,
        rust,
        mbx_tool_env(rust),
    )?
    .ok_or_else(|| {
        velnor_actions_workflow_steps::RenderError::InvalidWorkflow("mbx_steps_missing".to_owned())
    })
}

#[test]
fn native_action_installs_exact_version_and_owns_object_cache() {
    let [preflight, action, version_check] =
        steps(TEST_MBX_VERSION, TEST_RUST_TOOLCHAIN).expect("MBX steps");
    assert_eq!(preflight.name, MBX_PREFLIGHT_NAME);
    assert_eq!(
        version_check.name,
        velnor_actions_workflow_cache::cache_steps::MBX_VERSION_CHECK_NAME
    );
    let StepKind::Shell {
        run,
        env: check_env,
    } = version_check.kind
    else {
        panic!("version check must be a shell step");
    };
    assert!(run[2].contains("mbx --version"));
    assert!(run[2].contains("mbx 1.21.1"));
    assert!(run[2].contains("unterminated"));
    assert!(run[2].contains(&format!(
        "mise --no-config --no-env --no-hooks exec rust@{TEST_RUST_TOOLCHAIN} -- mbx --version"
    )));
    let StepKind::Action { uses, with, env } = action.kind else {
        panic!("restore must be an action");
    };
    let mut expected_check_env = env.clone();
    expected_check_env.extend(credential_scrub());
    assert_eq!(
        check_env, expected_check_env,
        "PATH check inherits action-owned settings and adds only the fixed shell credential scrub"
    );
    assert_eq!(uses, MBX_ACTION);
    assert_eq!(
        with.get("github-cache-mode").map(String::as_str),
        Some("objects")
    );
    assert_eq!(
        with.get("toolchain").map(String::as_str),
        Some(TEST_RUST_TOOLCHAIN)
    );
    assert_eq!(
        with.get("version").map(String::as_str),
        Some(TEST_MBX_VERSION)
    );
    assert!(!with.contains_key("cache-key-suffix"));
    assert!(!with.contains_key("isolate-objects-cache"));
    assert!(!with.contains_key("cache-key"));
    assert!(!with.contains_key("restore-keys"));
    let expected_generation = format!(
        "{}-gc-auto-v1-action-{}-lane-${{{{ runner.environment }}}}-job-${{{{ github.job }}}}",
        mbx_cache_generation(TEST_MBX_VERSION),
        "a".repeat(40)
    );
    assert_eq!(
        with.get("cache-generation").map(String::as_str),
        Some(expected_generation.as_str()),
        "the provider's default Rust compiler hash stays intact while cache-generation separates jobs"
    );
    assert_eq!(
        env.get(MBX_CACHE_MODE_ENV).map(String::as_str),
        Some(
            "${{ github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && 'write' || 'read' }}"
        )
    );
    assert_eq!(
        env.get("MBX_CACHE_DIR").map(String::as_str),
        Some("${{ runner.temp }}/velnor/mbx"),
        "action main and post use the runner-private stable logical store path"
    );
    assert_eq!(env.get("RUSTUP_HOME"), env.get("MISE_RUSTUP_HOME"));
    assert_eq!(env.get("CARGO_HOME"), env.get("MISE_CARGO_HOME"));
    for key in [
        "save-on-pull-request",
        "save-on-workflow-dispatch",
        "save-on-protected-branch",
    ] {
        assert_eq!(with.get(key).map(String::as_str), Some("false"));
    }
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
        .position(|name| name == "Restore MBX objects")
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
    assert!(text.contains("MBX_CACHE_DIR: ${{ runner.temp }}/velnor/mbx"));
    assert!(text.contains("MBX_GC_AUTO: \"1\""));
    Ok(())
}
