// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_workflow_renderer::cache_prune::{
    TOOLS_PRUNE_PREFIX, TOOLS_RETAINED_ENTRIES, prune_script, prune_targets,
};

#[test]
fn tools_family_has_a_retained_prune_target() {
    let targets = prune_targets();
    assert!(
        targets
            .iter()
            .any(|(prefix, keep)| prefix == TOOLS_PRUNE_PREFIX && *keep == TOOLS_RETAINED_ENTRIES),
        "unexpected prune targets: {targets:?}"
    );
}

#[test]
fn prune_script_emits_tools_retention() {
    let script = prune_script();
    let invocation = format!("prune '{TOOLS_PRUNE_PREFIX}' {TOOLS_RETAINED_ENTRIES}");
    assert!(
        script.contains(&invocation),
        "prune script is missing the tools retention call"
    );
}

#[test]
fn rendered_workflow_hands_the_head_branch_to_the_script_through_env()
-> Result<(), Box<dyn std::error::Error>> {
    let file = velnor_actions_workflow_renderer::cache_prune::render_cache_prune_yml(
        "ubuntu-26.04",
        "main",
        "CI",
        "0.0.0",
    )?;
    let yaml = file.bytes;
    assert!(
        yaml.contains("          HEAD_BRANCH: ${{ github.event.workflow_run.head_branch }}\n"),
        "head branch is not mapped into the step environment"
    );
    let run = yaml
        .lines()
        .find(|line| line.trim_start().starts_with("run: "))
        .ok_or("rendered workflow has no run step")?;
    assert!(
        !run.contains("${{"),
        "event data must not be interpolated into the shell body"
    );
    Ok(())
}

#[test]
fn maintenance_jobs_use_linux_without_changing_repository_targets()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::{cache_cleanup, cache_prune};
    for runner in ["ubuntu-24.04", "macos-15", "windows-2025"] {
        let prune = cache_prune::render_cache_prune_yml(runner, "main", "CI", "1.0.0")?;
        let cleanup = cache_cleanup::render_cache_cleanup_yml(runner, "1.0.0")?;
        let prune = prune.bytes;
        let cleanup = cleanup.bytes;
        assert!(prune.contains("runs-on: ubuntu-24.04"));
        assert!(cleanup.contains("runs-on: ubuntu-24.04"));
        assert!(prune.contains("GH_REPO: ${{ github.repository }}"));
        assert!(cleanup.contains("GH_REPO: ${{ github.repository }}"));
        assert!(prune.contains(TOOLS_PRUNE_PREFIX));
        assert!(cleanup.contains("refs/pull/${{ github.event.pull_request.number }}/merge"));
    }
    assert!(cache_prune::render_cache_prune_yml("bad label", "main", "CI", "1.0.0").is_err());
    assert!(cache_cleanup::render_cache_cleanup_yml("bad label", "1.0.0").is_err());
    Ok(())
}
