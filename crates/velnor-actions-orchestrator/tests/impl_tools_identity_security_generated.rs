use std::path::Path;

use super::{RUST_VERSION, ResolverStep, TestResult};
use crate::impl_common::write_fixture_mise;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::RenderedTree;

// The workflow fixture declares Rust, MBX, and Nextest, including two lock
// identities and a job-level Rustup handoff.
const WORKFLOW_OUTPUTS: [(&str, &str); 6] = [
    ("RUSTUP_TOOLCHAIN", RUST_VERSION),
    ("VELNOR_TOOL_MR_BOXINGTON", "1.24.1"),
    ("VELNOR_TOOL_NEXTEST", "0.9.148"),
    ("VELNOR_TOOL_NEXTEST_LOCK", "unlocked"),
    ("VELNOR_TOOL_RUST", RUST_VERSION),
    ("VELNOR_TOOL_RUST_LOCK", "unlocked"),
];

// This fixture has no Mise lock and its Rust lane emits only this tool value.
const SHARED_LANE_OUTPUTS: [(&str, &str); 1] = [("VELNOR_TOOL_RUST", RUST_VERSION)];

fn resolver_hostile_env<'a>(
    selected: &'a Path,
    xdg: &'a Path,
    home: &'a Path,
    bash_env: &'a Path,
    hostile_function: &'a str,
) -> [(&'static str, &'a Path); 14] {
    [
        ("MISE_DATA_DIR", selected),
        ("XDG_DATA_HOME", xdg),
        ("HOME", home),
        ("BASH_ENV", bash_env),
        ("BASH_FUNC_env%%", Path::new(hostile_function)),
        ("BASH_FUNC_sh%%", Path::new(hostile_function)),
        ("GITHUB_TOKEN", Path::new("runner-token-canary")),
        (
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            Path::new("runner-oidc-token-canary"),
        ),
        (
            "ACTIONS_ID_TOKEN_REQUEST_URL",
            Path::new("runner-oidc-url-canary"),
        ),
        ("ACTIONS_RUNTIME_TOKEN", Path::new("runtime-token-canary")),
        ("MISE_GITHUB_TOKEN", Path::new("mise-token-canary")),
        ("GH_TOKEN", Path::new("gh-token-canary")),
        ("GH_HOST", Path::new("host-canary")),
        ("GH_CONFIG_DIR", Path::new("config-canary")),
    ]
}

pub(super) fn assert_generated_workflow_resolver() -> TestResult {
    let tree = super::rendered_tree()?;
    let resolvers = super::generated_resolvers(&tree);
    let (path, step) = resolvers
        .iter()
        .find(|(path, _)| path.starts_with(".github/workflows/"))
        .ok_or("generated workflow has no project resolver")?;
    super::assert_step_envelope(&tree, path, step)?;
    execute_hostile_resolver(step, &WORKFLOW_OUTPUTS)
}

pub(super) fn assert_generated_shared_lane_composite_resolver() -> TestResult {
    let repo = crate::impl_step_role_name_authority::authority_fixture()?;
    std::fs::write(
        repo.path().join("mise.toml"),
        super::super::mise_toml("0.9.148", "20.1.0", "a"),
    )?;
    let tree: RenderedTree = render_staged_tree(&prepare(repo.path())?)?;
    let resolvers = super::generated_resolvers(&tree);
    let (path, step) = resolvers
        .iter()
        .find(|(path, _)| path.starts_with(".github/actions/") && path.ends_with("/action.yml"))
        .ok_or("generated shared-lane composite has no project resolver")?;
    let yaml = tree
        .get(path)
        .ok_or("generated shared-lane composite disappeared")?;
    super::assert_step_envelope(&tree, path, step)?;
    assert!(
        path.starts_with(".github/actions/") && yaml.contains("runs:\n  using: composite"),
        "resolver must come from a generated shared-lane composite"
    );
    execute_hostile_resolver(step, &SHARED_LANE_OUTPUTS)
}

fn execute_hostile_resolver(step: &ResolverStep, expected_outputs: &[(&str, &str)]) -> TestResult {
    let root = super::Scratch::new()?;
    let checkout = root.path().join("checkout");
    std::fs::create_dir_all(&checkout)?;
    std::fs::write(
        checkout.join("mise.toml"),
        super::super::mise_toml("0.9.148", "20.1.0", "a"),
    )?;
    let physical_parent = root.path().join("physical");
    let alias_parent = root.path().join("alias");
    let selected = physical_parent.join("mise-data");
    let selected_alias = alias_parent.join("mise-data");
    let xdg = root.path().join("xdg-decoy");
    let home = root.path().join("home-decoy");
    let home_data = home.join(".local/share/mise");
    std::fs::create_dir_all(&physical_parent)?;
    std::os::unix::fs::symlink(&physical_parent, &alias_parent)?;
    write_fixture_mise(&selected)?;
    write_fixture_mise(&xdg.join("mise"))?;
    write_fixture_mise(&home_data)?;
    let poison = super::poison_path(root.path())?;
    let (startup_marker, bash_env) = super::hostile_controls(root.path())?;
    let hostile_function = format!("() {{ printf pwned >> '{}'; }}", startup_marker.display());
    let extras = resolver_hostile_env(&selected_alias, &xdg, &home, &bash_env, &hostile_function);
    let output = super::run_step(root.path(), &checkout, step, &poison, &extras)?;
    assert_success_and_no_hostile_execution(root.path(), step, &output, &startup_marker);
    super::assert_audit(root.path(), &selected)?;
    super::assert_outputs(root.path(), expected_outputs)
}

fn assert_success_and_no_hostile_execution(
    root: &Path,
    step: &ResolverStep,
    output: &std::process::Output,
    startup_marker: &Path,
) {
    assert!(
        output.status.success(),
        "{}: {}",
        step.shell,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!startup_marker.exists(), "startup code or function ran");
    assert!(!root.join("poison.log").exists(), "poison PATH ran");
}
