//! Typed verification task routing through schema-2 execution modes.

use velnor_actions_orchestrator_generation::generate::render_staged_tree;
use velnor_actions_orchestrator_generation::prepare::prepare;

use crate::impl_common::{TestResult, make_repo};
use crate::impl_schema2_routing::{job_body, required_file};

const SCALE_RUNS: &str = "runs-on: [velnor, ubuntu-26.04-scale-set, verification-worker]";

#[test]
fn verification_tasks_follow_eligible_lanes_and_stay_required() -> TestResult {
    for mode in ["hosted", "scale-set", "both"] {
        let repo = make_repo(&config(mode))?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let workflow = required_file(&tree, ".github/workflows/ci.yml")?;
        let required = job_body(workflow, "required")?;
        let actionlint = job_body(workflow, "actionlint")?;
        let macos = job_body(workflow, "task-native-format")?;
        assert!(
            actionlint.contains("runs-on: ubuntu-26.04"),
            "{mode}: {actionlint}"
        );
        assert!(
            !workflow.contains("  actionlint__hosted:"),
            "{mode}: {workflow}"
        );
        assert!(
            !workflow.contains("  actionlint__local:"),
            "{mode}: {workflow}"
        );
        assert!(macos.contains("runs-on: macos-15"), "{mode}: {macos}");
        assert!(
            !workflow.contains("  task-native-format__hosted:"),
            "{mode}: {workflow}"
        );
        assert!(
            !workflow.contains("  task-native-format__local:"),
            "{mode}: {workflow}"
        );
        assert!(
            required.contains("- task-native-format"),
            "{mode}: {required}"
        );

        match mode {
            "hosted" => {
                let linux = job_body(workflow, "task-linux-lint")?;
                assert!(linux.contains("runs-on: ubuntu-26.04"), "{linux}");
                assert!(required.contains("- task-linux-lint"), "{required}");
                assert!(!workflow.contains("  task-linux-lint__hosted:"));
                assert!(!workflow.contains("  task-linux-lint__local:"));
            }
            "scale-set" => {
                let linux = job_body(workflow, "task-linux-lint")?;
                assert!(linux.contains(SCALE_RUNS), "{linux}");
                assert!(required.contains("- task-linux-lint"), "{required}");
                assert!(!workflow.contains("  task-linux-lint__hosted:"));
                assert!(!workflow.contains("  task-linux-lint__local:"));
            }
            "both" => {
                let hosted = job_body(workflow, "task-linux-lint__hosted")?;
                let scale = job_body(workflow, "task-linux-lint__local")?;
                assert!(hosted.contains("runs-on: ubuntu-26.04"), "{hosted}");
                assert!(scale.contains(SCALE_RUNS), "{scale}");
                assert!(required.contains("- task-linux-lint__hosted"), "{required}");
                assert!(required.contains("- task-linux-lint__local"), "{required}");
                assert!(!workflow.contains("  task-linux-lint:"));
            }
            _ => unreachable!(),
        }
    }
    Ok(())
}

#[test]
fn macos_task_cannot_select_the_linux_scale_set_profile() -> TestResult {
    let config = format!(
        "{}\n[execution.overrides.\"task-native-format\"]\nprofile = \"local\"\nrole = \"verification\"",
        config("hosted")
    );
    let repo = make_repo(&config)?;
    let prep = prepare(repo.path())?;
    let error = render_staged_tree(&prep).expect_err("macOS cannot target Linux Scale Set");
    assert!(
        error
            .to_string()
            .contains("verification_runner_incompatible_with_scale_set"),
        "{error}"
    );
    Ok(())
}

fn config(mode: &str) -> String {
    format!(
        "schema = 2\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
[[workflow.tasks]]\nid = \"linux-lint\"\nkind = \"verification\"\nmise_task = \"lint-linux\"\nrunner = \"linux-x64\"\ntimeout_minutes = 10\n\
[[workflow.tasks]]\nid = \"native-format\"\nkind = \"verification\"\nmise_task = \"desktop-format-check\"\nrunner = \"macos-arm64\"\ntimeout_minutes = 10\n\
[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nmode = \"{mode}\"\n\
[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\
[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"ubuntu-26.04-scale-set\", \"velnor\", \"verification-worker\"]\nplatform = \"linux/amd64\""
    )
}
