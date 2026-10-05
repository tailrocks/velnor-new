//! End-to-end credential-scrub case over tempdir fixtures.
//!
//! Runs the real `prepare` → `render_staged_tree` path and asserts the
//! emitted workflow text scrubs ambient auth from repo-code steps.

use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::WORKFLOW_PATH;

use crate::impl_common::{TestResult, config_with_branch, make_repo};

/// Hand-written lock for a no-deps fixture package: hermetic, no network.
fn demo_lock(name: &str) -> String {
    format!("version = 4\n\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n")
}

#[test]
fn emitted_yaml_scrubs_repo_code_steps() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or("missing workflow in staged tree")?;
    // Obligation wrappers execute repository code: `sh -c` scripts
    // carry the `unset` prelude naming every unset var, and the scrub
    // overlay blanks inheritance.
    let prelude = velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude();
    assert!(yaml.contains(&prelude), "scripts must unset:\n{yaml}");
    for var in velnor_actions_workflow_renderer::toolchain_env::CREDENTIAL_UNSET_VARS {
        assert!(prelude.contains(var), "prelude must unset {var}");
    }
    assert!(
        yaml.contains("GITHUB_TOKEN: \"\""),
        "steps must scrub:\n{yaml}"
    );
    Ok(())
}

/// Steps that execute no repository code keep ambient auth: no scrub
/// overlay, no unset wrapper. Scrubbing them broke tool bootstrap
/// (mise `ubi:` 401, zizmor empty-token abort, CI run 36815180228).
#[test]
fn emitted_yaml_keeps_ambient_auth_steps_unscrubbed() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    // Lockful: fetch steps only emit when a lockfile selects them.
    std::fs::write(repo.path().join("Cargo.lock"), demo_lock("demo"))?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or("missing workflow in staged tree")?;
    for name in [
        "Prepare pinned tools",
        "Prepare Rust components",
        "Fetch Cargo sources",
    ] {
        let block = step_block(yaml, name).ok_or_else(|| format!("missing ambient step {name}"))?;
        assert!(
            !block.contains("GITHUB_TOKEN: \"\""),
            "{name} must not scrub:\n{block}"
        );
        assert!(
            !block.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
            "{name} must not unset:\n{block}"
        );
        assert!(
            !block.contains("run: \"env -u "),
            "{name} must not wrap argv:\n{block}"
        );
    }
    Ok(())
}

/// Extract one `- name:` step block from emitted workflow text.
fn step_block<'a>(yaml: &'a str, name: &str) -> Option<&'a str> {
    let start = yaml.find(&format!("- name: {name}\n"))?;
    let tail = &yaml[start..];
    let end = tail["- name: ".len()..]
        .find("- name: ")
        .map_or(tail.len(), |at| at + "- name: ".len());
    Some(tail[..end].trim_end())
}
