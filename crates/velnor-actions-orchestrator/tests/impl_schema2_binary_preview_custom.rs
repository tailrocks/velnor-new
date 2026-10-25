use super::{binary_repo, tap_enabled_config};
use crate::impl_common::{TestResult, git};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

fn custom_tap_enabled_config() -> Result<String, Box<dyn std::error::Error>> {
    let config = tap_enabled_config(false);
    let native_spec = config
        .find("\n[distribution.homebrew_formula]\n")
        .ok_or("native formula spec section")?;
    Ok(format!(
        "{}\n\
[distribution.homebrew_tap.updater]\n\
task = \"tap:update\"\n\
source = {{ mise_config = \"mise.toml\", working_directory = \".\" }}\n\
runner = {{ label = \"ubuntu-26.04\", platform = \"linux_x64\", executor = \"hosted\" }}\n\
inputs = [\"mise.toml\", \"Formula/demo-preview.rb\"]\n\
tools = [\"rust\"]\n\
timeout_minutes = 5\n\
\n[[checks]]\n\
id = \"tap-update\"\n\
task = \"tap:update\"\n\
directory = \".\"\n\
inputs = [\"mise.toml\", \"Formula/demo-preview.rb\"]\n\
tools = [\"rust\"]\n\
timeout_minutes = 5\n\
[checks.runner]\n\
label = \"ubuntu-26.04\"\n\
platform = \"linux_x64\"\n\
executor = \"hosted\"\n",
        &config[..native_spec]
    ))
}

#[test]
fn custom_tap_generation_keeps_legacy_updater_without_native_source_policy() -> TestResult {
    let repo = binary_repo(&custom_tap_enabled_config()?)?;
    let formula = repo.path().join("Formula/demo-preview.rb");
    std::fs::create_dir_all(formula.parent().ok_or("formula parent missing")?)?;
    std::fs::write(&formula, "class DemoPreview < Formula\nend\n")?;
    let mise = repo.path().join("mise.toml");
    let mut mise_config = std::fs::read_to_string(&mise)?;
    mise_config.push_str("\n[tasks.\"tap:update\"]\nrun = 'true'\n");
    std::fs::write(&mise, mise_config)?;
    git(
        &["add", "Formula/demo-preview.rb", "mise.toml"],
        repo.path(),
    )?;

    let source_policy = repo.path().join(".velnor/homebrew-release-policy.json");
    assert!(!source_policy.exists());
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let preview = tree
        .get(".github/workflows/binary-preview.yml")
        .ok_or("preview workflow missing")?;
    assert!(preview.contains("VELNOR_INTERNAL_OP: prepare-homebrew-tap-v1"));
    assert!(!preview.contains("VELNOR_HOMEBREW_FORMULA_SPEC"));
    assert!(!source_policy.exists());
    Ok(())
}
