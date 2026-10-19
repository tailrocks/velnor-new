//! Do a check of validator configuration and Required job dependencies.

use velnor_actions_contract::ValidatorKind;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::FINAL_JOB_ID;

use crate::impl_common::{TestResult, config_with_branch, make_repo};

#[test]
fn configured_zizmor_is_rendered_and_required() -> TestResult {
    let config = format!(
        "{}\n[workflow.verify]\njobs = [\"zizmor\"]\n",
        config_with_branch()
    );
    let repo = make_repo(&config)?;
    let prep = prepare(repo.path())?;
    assert_eq!(
        prep.workflow
            .support
            .as_ref()
            .ok_or("The support workflow is not available.")?
            .validators,
        [ValidatorKind::Zizmor],
        "The configuration must contain the validator."
    );
    let command = prep
        .workflow
        .context
        .validator_commands
        .iter()
        .find(|command| command.validator == ValidatorKind::Zizmor)
        .ok_or("The Zizmor command is not available.")?;
    assert_eq!(command.name, "Run zizmor");
    assert!(command.argv.iter().any(|argument| argument == "zizmor"));

    let tree = render_staged_tree(&prep)?;
    let workflow = tree
        .get(".github/workflows/ci.yml")
        .ok_or("The workflow is not available.")?;
    assert!(
        workflow.contains("  zizmor:\n"),
        "The workflow has no Zizmor job: {workflow}"
    );
    let required = workflow
        .split_once(&format!("  {FINAL_JOB_ID}:\n"))
        .map(|(_, rest)| rest)
        .ok_or("The Required job is not available.")?;
    let lines: Vec<_> = required.lines().collect();
    let needs_index = lines
        .iter()
        .position(|line| line.starts_with("    needs:"))
        .ok_or("The Required job has no needs field.")?;
    let needs: Vec<_> = lines
        .iter()
        .skip(needs_index + 1)
        .take_while(|line| line.starts_with("      - "))
        .copied()
        .collect();
    assert!(
        needs.iter().any(|line| line.trim() == "- zizmor"),
        "The Required job must wait for zizmor: {needs:?}"
    );
    Ok(())
}
