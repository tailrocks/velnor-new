use super::{TestResult, WORKFLOW_PATH};
use velnor_actions_orchestrator::GenerationPreparation;
use velnor_actions_workflow_renderer::RenderedTree;

/// Inspect the actual provider job and its action/guard/reporter ordering.
pub(super) fn assert_provider_save_budget(
    prep: &GenerationPreparation,
    tree: &RenderedTree,
) -> TestResult {
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or("missing workflow in staged tree")?;
    let body = yaml
        .split("  tofu-stacks-a:\n")
        .nth(1)
        .ok_or("rendered tofu job")?;
    let body = body
        .lines()
        .take_while(|line| !line.starts_with("  ") || line.starts_with("   "))
        .collect::<Vec<_>>()
        .join("\n");
    let admission = yaml
        .find("id: velnor-optional-providers-save-budget")
        .ok_or("provider admission")?;
    let save = yaml
        .find("id: velnor-optional-providers-save\n")
        .ok_or("provider save")?;
    let reporter = yaml
        .find("steps.velnor-optional-providers-save.outcome == 'failure'")
        .ok_or("provider reporter")?;
    assert!(admission < save && save < reporter);
    let save_start = yaml[..save]
        .rfind("name: Save Tofu providers")
        .ok_or("provider action start")?;
    let save_body = &yaml[save_start..reporter];
    for required in [
        "timeout-minutes: 1",
        "continue-on-error: true",
        "steps.velnor-optional-providers-save-budget.outputs.admitted",
    ] {
        assert!(
            save_body.contains(required),
            "missing provider bound: {required}"
        );
    }
    assert!(body.contains("Record optional cache budget"));
    assert_jactionlint_verifier_calls(prep, tree, yaml)?;
    Ok(())
}

pub(super) fn assert_jactionlint_verifier_calls(
    prep: &GenerationPreparation,
    tree: &RenderedTree,
    yaml: &str,
) -> TestResult {
    let action_calls = yaml
        .matches("uses: ./.github/actions/velnor-verify-jactionlint")
        .count();
    let action_definition = tree
        .get(".github/actions/velnor-verify-jactionlint/action.yml")
        .ok_or("generated jactionlint verifier action missing")?;
    let lines = yaml.lines().collect::<Vec<_>>();
    let verifier_steps = lines
        .iter()
        .filter(|line| line.trim_start() == "- name: Verify jactionlint")
        .count();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed != "- name: Verify jactionlint" {
            continue;
        }
        let indent = line.len() - trimmed.len();
        let mut fields = vec![trimmed.to_owned()];
        for next in lines.iter().skip(index + 1) {
            let next_trimmed = next.trim_start();
            let next_indent = next.len() - next_trimmed.len();
            if next_indent < indent || (next_indent == indent && next_trimmed.starts_with("- ")) {
                break;
            }
            fields.push(next_trimmed.to_owned());
        }
        assert_eq!(
            fields.len(),
            2,
            "verifier call has only name and uses: {fields:?}"
        );
        assert_eq!(
            fields[1],
            "uses: ./.github/actions/velnor-verify-jactionlint # zizmor: ignore[self-repository]"
        );
    }
    assert!(verifier_steps > 0, "jactionlint verifier step: {yaml}");
    assert_eq!(
        action_calls, verifier_steps,
        "one typed verifier call per exact-name step: {yaml}"
    );
    assert!(
        yaml.lines()
            .all(|line| !line.trim_start().starts_with("shell:")),
        "ordinary workflow steps keep the default shell: {yaml}"
    );
    assert!(
        action_definition.lines().any(|line| {
            line.trim()
                == format!(
                    "shell: {}",
                    velnor_actions_workflow_renderer::VERIFY_VERIFIER_SHELL
                )
        }),
        "canonical jactionlint action retains the fixed verifier shell"
    );
    assert_jactionlint_action_definition(prep, tree)?;
    Ok(())
}

fn assert_jactionlint_action_definition(
    prep: &velnor_actions_orchestrator::GenerationPreparation,
    tree: &velnor_actions_workflow_renderer::render::RenderedTree,
) -> TestResult {
    let catalog = prep.emission_catalog();
    let version = catalog.version(velnor_actions_mise::PinnedTool::Jactionlint);
    let script = velnor_actions_workflow_renderer::jactionlint_verifier_script(version);
    let verifier = velnor_actions_workflow_renderer::steps::jactionlint_verifier_step(
        "Verify jactionlint",
        vec!["sh".to_owned(), "-c".to_owned(), script],
        std::collections::BTreeMap::new(),
    )?;
    let expected = velnor_actions_workflow_renderer::verifier_composite::render_verifier_composite(
        velnor_actions_contract::workflow::JACTIONLINT_VERIFIER_USES,
        &verifier,
        env!("CARGO_PKG_VERSION"),
    )?;
    let actual = tree
        .get(&expected.path)
        .ok_or("missing Jactionlint action")?;
    assert_eq!(actual, expected.bytes);
    Ok(())
}
