//! Hard decision 4: tool versions come from the project's `mise.toml`.
//!
//! A version the project's Mise config declares never appears in generated
//! text; the generated resolver step reads it at run time, so bumping it in
//! `mise.toml` needs no regeneration. Tools the project does not declare
//! keep the Velnor pin.

use std::fs;

use velnor_actions_mise::{ACTIONLINT_VERSION, MR_BOXINGTON_VERSION, RUST_VERSION};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::RenderedTree;
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use crate::impl_common::{TestResult, config_with_branch, make_repo};

/// Consumer config selecting the MBX driver and the Nextest runner.
const MBX_NEXTEST_CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.rust]\ncompile_driver = \"mbx\"\ntest_runner = \"cargo_nextest\"\n";

/// Project config declaring Rust (generation inventory runs it locally,
/// so it is the generator pin), MBX, and Nextest at non-Velnor versions.
fn mise_toml(mbx: &str, nextest: &str) -> String {
    format!(
        "[tools]\nrust = \"{RUST_VERSION}\"\nmr-boxington = \"{mbx}\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"{nextest}\"\n"
    )
}

fn render(mbx: &str, nextest: &str) -> Result<RenderedTree, Box<dyn std::error::Error>> {
    let repo = make_repo(MBX_NEXTEST_CONFIG)?;
    fs::write(repo.path().join("mise.toml"), mise_toml(mbx, nextest))?;
    Ok(render_staged_tree(&prepare(repo.path())?)?)
}

fn texts(tree: &RenderedTree) -> Vec<(String, String)> {
    tree.files
        .iter()
        .map(|file| (file.path.clone(), file.bytes.clone()))
        .collect()
}

#[test]
fn project_declared_versions_never_appear_in_generated_text() -> TestResult {
    let tree = render("1.30.7", "0.9.977")?;
    for (path, text) in texts(&tree) {
        for literal in [
            "1.30.7",
            "0.9.977",
            &format!("rust@{RUST_VERSION}"),
            &format!("RUSTUP_TOOLCHAIN: {RUST_VERSION}"),
            &format!("{RUST_VERSION}-x86_64"),
        ] {
            assert!(
                !text.contains(literal),
                "{path} copies `{literal}`:\n{text}"
            );
        }
    }
    let ci = tree.get(WORKFLOW_PATH).ok_or("missing ci.yml")?;
    assert!(ci.contains("Resolve project tool versions"), "{ci}");
    assert!(ci.contains("velnor_tool VELNOR_TOOL_RUST '"), "{ci}");
    assert!(
        ci.contains("velnor_tool VELNOR_TOOL_MR_BOXINGTON '"),
        "{ci}"
    );
    assert!(ci.contains("velnor_tool VELNOR_TOOL_NEXTEST '"), "{ci}");
    // The resolver reads stored values only: no project config is loaded.
    assert!(
        ci.contains("mise --no-config --no-env --no-hooks config get"),
        "{ci}"
    );
    assert!(
        ci.contains(r#"'rust@'\"${VELNOR_TOOL_RUST}\""#),
        "commands name the run-time reference: {ci}"
    );
    assert!(
        ci.contains("${{ env.VELNOR_TOOL_RUST }}"),
        "cache keys and action inputs read the resolved version: {ci}"
    );
    Ok(())
}

#[test]
fn bumping_project_versions_needs_no_regeneration() -> TestResult {
    let before = render("1.30.7", "0.9.977")?;
    let after = render("1.31.0", "0.9.978")?;
    assert_eq!(
        texts(&before),
        texts(&after),
        "a mise.toml version bump must not change any generated file"
    );
    Ok(())
}

#[test]
fn undeclared_tools_keep_the_velnor_pin() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = tree.get(WORKFLOW_PATH).ok_or("missing ci.yml")?;
    assert!(ci.contains(&format!("rust@{RUST_VERSION}")), "{ci}");
    assert!(!ci.contains("VELNOR_TOOL_"), "{ci}");
    assert!(!ci.contains("Resolve project tool versions"), "{ci}");
    let declared = render(MR_BOXINGTON_VERSION, "0.9.977")?;
    let ci = declared.get(WORKFLOW_PATH).ok_or("missing ci.yml")?;
    // actionlint is velnor-internal here: the project declares no version.
    assert!(
        ci.contains(&format!("actionlint@{ACTIONLINT_VERSION}")),
        "{ci}"
    );
    // A declared tool is a reference even when it equals the Velnor pin.
    assert!(
        !ci.contains(&format!("mr-boxington@{MR_BOXINGTON_VERSION}")),
        "{ci}"
    );
    Ok(())
}
