//! Crate-job preflight vs the repository `mr_boxington = true` rust record.
//!
//! E2E proof for the mise-managed MBX wrapper pin: with the option set,
//! the generated `Prepare pinned tools` install line must carry the
//! repository's own `mr-boxington` pin (mise's exec-time wrapper
//! dispatches cargo through it); without the option the install lines
//! must stay free of `mr-boxington@` (action-owned only).

use std::fs;

use velnor_actions_mise::PinnedTool;
use velnor_actions_orchestrator::{GenerationPreparation, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use crate::impl_common::{TestResult, config_with_branch, make_repo};

/// Repo mise.toml opting cargo into the mise-managed MBX wrapper.
const MISE_TOML_WITH_WRAPPER: &str = "\
[tools]
rust = { version = \"1.94.1\", mr_boxington = true }
mr-boxington = \"1.24.0\"
";

/// Staged `ci.yml` text for one prepared repository.
fn staged_ci(prep: &GenerationPreparation) -> Result<String, Box<dyn std::error::Error>> {
    let tree = render_staged_tree(prep)?;
    Ok(tree
        .get(WORKFLOW_PATH)
        .ok_or("missing workflow")?
        .to_owned())
}

/// Every generated config-less `mise install` run line carries at most the
/// exact repository pin, and at least one line (the crate jobs' preflight)
/// carries it.
fn assert_preflight_installs_the_repo_pin(yaml: &str) {
    let mut pinned_lines = 0;
    for line in yaml
        .lines()
        .filter(|line| line.contains("no-hooks install"))
    {
        let pins: Vec<&str> = line
            .split_whitespace()
            .filter(|word| word.starts_with("mr-boxington@"))
            .collect();
        assert!(
            pins.iter().all(|pin| *pin == "mr-boxington@1.24.0"),
            "only the repository mr-boxington pin may install: {line}"
        );
        pinned_lines += usize::from(!pins.is_empty());
    }
    assert!(
        pinned_lines > 0,
        "the crate preflight must install the repo mr-boxington pin: {yaml}"
    );
}

#[test]
fn crate_preflight_installs_the_repo_mr_boxington_pin_for_the_wrapper_option() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    fs::write(repo.path().join("mise.toml"), MISE_TOML_WITH_WRAPPER)?;
    let prep = prepare(repo.path())?;
    assert!(
        prep.catalog.rust_mr_boxington(),
        "the wrapper option must reach the resolved catalog"
    );
    assert_eq!(
        prep.catalog.version(PinnedTool::MrBoxington),
        "1.24.0",
        "the resolved catalog must pin the repository's own mr-boxington"
    );
    let yaml = staged_ci(&prep)?;
    assert_preflight_installs_the_repo_pin(&yaml);
    Ok(())
}

#[test]
fn crate_preflight_stays_without_mr_boxington_without_the_wrapper_option() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    assert!(
        !prep.catalog.rust_mr_boxington(),
        "no option means no wrapper preflight"
    );
    let yaml = staged_ci(&prep)?;
    assert!(
        !yaml.contains("mr-boxington@"),
        "without the option mr-boxington stays action-owned: {yaml}"
    );
    Ok(())
}

/// Tofu-only fixture: git init, config with the tofu root, fixture
/// manifest, and one `main.tf` so the root selects.
fn make_tofu_only_wrapper_repo() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    use crate::impl_common::{git, install_fixture_release_manifest};
    let dir = tempfile::TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(
        root.join(".velnor/config.toml"),
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\".\"]\n",
    )?;
    install_fixture_release_manifest(root)?;
    fs::write(root.join("main.tf"), "")?;
    fs::write(root.join("mise.toml"), MISE_TOML_WITH_WRAPPER)?;
    Ok(dir)
}

#[test]
fn pure_tofu_lanes_generate_without_the_repo_mr_boxington_pin() -> TestResult {
    let repo = make_tofu_only_wrapper_repo()?;
    let prep = prepare(repo.path())?;
    assert!(
        prep.catalog.rust_mr_boxington(),
        "the wrapper option is repo-wide, even with no rust stack"
    );
    // The pin must not leak into pure-tofu jobs: without the fix the
    // prepare step carried `mr-boxington@` with no job MBX action and
    // generation aborted on `mbx_mise_selector_forbidden`.
    let yaml = staged_ci(&prep)?;
    assert!(
        yaml.contains("opentofu@"),
        "the tofu lane must install its driver: {yaml}"
    );
    for line in yaml
        .lines()
        .filter(|line| line.contains("no-hooks install"))
    {
        assert!(
            !line.contains("mr-boxington@") && !line.contains(" rust@"),
            "pure-tofu preflights install neither rust nor the repo pin: {line}"
        );
    }
    Ok(())
}
