//! Typed verification task routing through schema-2 execution modes.

use std::fs;

use tempfile::TempDir;

use velnor_actions_orchestrator_generation::generate::render_staged_tree;
use velnor_actions_orchestrator_generation::prepare::prepare;

use crate::impl_common::{TestResult, config_with_branch, git, make_repo};
use crate::impl_schema2_routing::{job_body, required_file};

const SCALE_RUNS: &str = "runs-on: [velnor, ubuntu-26.04-scale-set, verification-worker]";

#[test]
fn cold_seed_actionlint_explicitly_installs_exact_tools() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let workflow = required_file(&tree, ".github/workflows/ci.yml")?;
    let actionlint = job_body(workflow, "actionlint")?;

    let checkout = actionlint.find("name: Checkout").expect("checkout");
    let install = actionlint
        .find("name: Prepare pinned tools")
        .expect("explicit install");
    let execution = actionlint.find("name: Run actionlint").expect("execution");
    assert!(checkout < install && install < execution, "{actionlint}");
    let install_line = "run: mise --no-config --no-env --no-hooks install \
                        actionlint@1.7.12 shellcheck@0.11.0";
    let execution_line = "run: mise --no-config --no-env --no-hooks exec \
                          actionlint@1.7.12 shellcheck@0.11.0 -- actionlint -color";
    assert!(
        actionlint.contains(install_line),
        "cold seed must install exact validators:\n{actionlint}"
    );
    assert!(
        actionlint.contains(execution_line),
        "execution must remain pinned and fail-closed:\n{actionlint}"
    );
    assert!(
        actionlint.contains("MISE_AUTO_INSTALL: \"false\""),
        "auto-install must stay disabled:\n{actionlint}"
    );
    assert!(
        actionlint.contains("MISE_EXEC_AUTO_INSTALL: \"false\""),
        "exec auto-install must stay disabled:\n{actionlint}"
    );
    Ok(())
}

#[test]
fn verification_tasks_follow_eligible_lanes_and_stay_required() -> TestResult {
    for mode in ["hosted", "scale-set", "both"] {
        let repo = make_repo(&config(mode))?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let workflow = required_file(&tree, ".github/workflows/ci.yml")?;
        let required = job_body(workflow, "required")?;
        let actionlint = job_body(workflow, "actionlint")?;
        let macos = job_body(workflow, "task-native-format")?;
        assert_no_output_task_sequence(macos, mode);
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
                assert!(!linux.contains("name: Acquire Velnor"), "{linux}");
                assert!(!linux.contains("name: Download plan"), "{linux}");
                assert!(required.contains("- task-linux-lint"), "{required}");
                assert!(!workflow.contains("  task-linux-lint__hosted:"));
                assert!(!workflow.contains("  task-linux-lint__local:"));
            }
            "scale-set" => {
                let linux = job_body(workflow, "task-linux-lint")?;
                assert!(linux.contains(SCALE_RUNS), "{linux}");
                assert!(!linux.contains("name: Acquire Velnor"), "{linux}");
                assert!(!linux.contains("name: Download plan"), "{linux}");
                assert!(required.contains("- task-linux-lint"), "{required}");
                assert!(!workflow.contains("  task-linux-lint__hosted:"));
                assert!(!workflow.contains("  task-linux-lint__local:"));
            }
            "both" => {
                let hosted = job_body(workflow, "task-linux-lint__hosted")?;
                let scale = job_body(workflow, "task-linux-lint__local")?;
                assert!(hosted.contains("runs-on: ubuntu-26.04"), "{hosted}");
                assert!(scale.contains(SCALE_RUNS), "{scale}");
                assert!(!hosted.contains("name: Acquire Velnor"), "{hosted}");
                assert!(!scale.contains("name: Acquire Velnor"), "{scale}");
                assert!(!hosted.contains("name: Download plan"), "{hosted}");
                assert!(!scale.contains("name: Download plan"), "{scale}");
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
fn velnor_lock_stages_the_task_runner_asset_before_output_steps() -> TestResult {
    let repo = make_velnor_repo("both", true)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let workflow = required_file(&tree, ".github/workflows/ci.yml")?;
    let action = required_file(&tree, ".github/actions/task-linux-lint/action.yml")?;
    assert_output_step_order(action, "Velnor lock / Both");
    assert!(
        action.contains("https://example.invalid/x86_64-unknown-linux-gnu"),
        "task asset must use its Linux x64 target: {action}"
    );
    for id in ["task-linux-lint__hosted", "task-linux-lint__local"] {
        let job = job_body(workflow, id)?;
        assert!(job.contains("./.github/actions/task-linux-lint"), "{job}");
    }
    let no_output = job_body(workflow, "task-native-format")?;
    assert_no_output_task_sequence(no_output, "Velnor lock / Both");
    Ok(())
}

#[test]
fn velnor_preseed_stages_the_output_task_before_plan_and_export() -> TestResult {
    let repo = make_velnor_repo("hosted", false)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let workflow = required_file(&tree, ".github/workflows/ci.yml")?;
    assert!(workflow.contains("x86_64-unknown-linux-gnu"), "{workflow}");
    let task = job_body(workflow, "task-linux-lint")?;
    let download = task
        .find("name: Download helper (pre-seed trust-on-review)")
        .expect("pre-seed helper download");
    let verify = task
        .find("name: Verify helper manifest (pre-seed trust-on-review)")
        .expect("pre-seed helper verification");
    let stage = task
        .find("name: Stage helper (pre-seed trust-on-review)")
        .expect("pre-seed helper stage");
    let plan = task.find("name: Download plan").expect("plan download");
    let run = task.find("mise run lint-linux").expect("task run");
    let export = task
        .find("name: Capture declared verification outputs")
        .expect("output exporter");
    assert!(download < verify && verify < stage && stage < plan && plan < run && run < export);
    assert!(!task.contains("name: Acquire Velnor"), "{task}");
    assert_no_output_task_sequence(job_body(workflow, "task-native-format")?, "Velnor pre-seed");
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

#[test]
fn verification_outputs_reuse_the_static_job_and_shared_plan_bound_exporter() -> TestResult {
    for mode in ["hosted", "scale-set", "both"] {
        let repo = make_repo(&config_with_outputs(mode))?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let workflow = required_file(&tree, ".github/workflows/ci.yml")?;
        let required = job_body(workflow, "required")?;
        let task_ids: &[&str] = if mode == "both" {
            &["task-linux-lint__hosted", "task-linux-lint__local"]
        } else {
            &["task-linux-lint"]
        };
        for id in task_ids {
            let job = job_body(workflow, id)?;
            assert!(job.contains("needs:\n      - plan"), "{mode}: {job}");
            assert!(required.contains(&format!("- {id}")), "{mode}: {required}");
        }
        let output_steps = if mode == "both" {
            required_file(&tree, ".github/actions/task-linux-lint/action.yml")?
        } else {
            job_body(workflow, "task-linux-lint")?
        };
        assert_output_step_order(output_steps, mode);
        assert!(
            !workflow.contains("artifact_hosted_matrix"),
            "{mode}: {workflow}"
        );
        assert!(
            !workflow.contains("artifact_velnor_matrix"),
            "{mode}: {workflow}"
        );
        assert!(!workflow.contains("artifact-build:"), "{mode}: {workflow}");
    }
    Ok(())
}

fn assert_output_step_order(steps: &str, mode: &str) {
    assert!(
        steps.contains("name: Acquire Velnor"),
        "{mode}: output sequence misses pinned helper acquire: {steps}"
    );
    assert!(steps.contains("name: Download plan"), "{mode}: {steps}");
    assert!(
        steps.contains("name: Capture declared verification outputs"),
        "{mode}: {steps}"
    );
    assert!(
        steps.contains("VELNOR_INTERNAL_OP: export-verification-artifact-v1"),
        "{mode}: {steps}"
    );
    assert!(
        steps.contains("name: ${{ steps.verification-artifact-export.outputs.artifact_name }}"),
        "{mode}: {steps}"
    );
    assert!(
        steps.contains("if-no-files-found: error"),
        "{mode}: {steps}"
    );
    assert!(steps.contains("cache: \"false\""), "{mode}: {steps}");
    assert!(steps.contains("cache_save: \"false\""), "{mode}: {steps}");
    assert!(!steps.contains("actions/cache/restore"), "{mode}: {steps}");
    assert!(!steps.contains("actions/cache/save"), "{mode}: {steps}");
    assert_eq!(
        steps.matches("mise run lint-linux").count(),
        1,
        "{mode}: {steps}"
    );
    let acquire = steps
        .find("name: Acquire Velnor")
        .expect("acquire included");
    let download = steps.find("name: Download plan").expect("plan download");
    let task_run = steps.find("mise run lint-linux").expect("single task run");
    let export = steps
        .find("name: Capture declared verification outputs")
        .expect("output exporter");
    assert!(acquire < download && download < task_run && task_run < export);
}

fn assert_no_output_task_sequence(job: &str, mode: &str) {
    assert!(!job.contains("name: Acquire Velnor"), "{mode}: {job}");
    assert!(!job.contains("name: Download plan"), "{mode}: {job}");
    assert!(
        !job.contains("name: Capture declared verification outputs"),
        "{mode}: {job}"
    );
    assert!(job.contains("cache: \"false\""), "{mode}: {job}");
    assert!(job.contains("cache_save: \"false\""), "{mode}: {job}");
    assert!(!job.contains("actions/cache/restore"), "{mode}: {job}");
    assert!(!job.contains("actions/cache/save"), "{mode}: {job}");
    let checkout = job.find("name: Checkout").expect("checkout");
    let mise = job.find("name: Setup Mise").expect("Mise setup");
    let install = job
        .find("name: Install locked task tools")
        .expect("locked install");
    let run = job
        .find("mise run desktop-format-check")
        .expect("declared legacy task");
    assert!(
        checkout < mise && mise < install && install < run,
        "{mode}: {job}"
    );
}

fn make_velnor_repo(mode: &str, with_lock: bool) -> Result<TempDir, Box<dyn std::error::Error>> {
    let mut config = config_with_outputs(mode).replace(
        "name = \"CI\"",
        "name = \"CI\"\npolicy = \"velnor-repository-v1\"",
    );
    config = config.replace(
        "[execution]",
        "[stacks.rust.policy]\nversion = \"0.1.3\"\nsha256 = \"104c0d8b3a827875776358f941aa88f1c5837c1009305076af9380f4e3fcda25\"\nprofile = \"rust-strict-v1\"\n\n[execution]",
    );
    let repo = make_repo(&config)?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ],
        repo.path(),
    )?;
    fs::write(
        repo.path().join(".velnor/version-policy.toml"),
        include_str!("../../../../.velnor/version-policy.toml"),
    )?;
    if with_lock {
        fs::write(repo.path().join(".velnor/generator.lock"), lock_text()?)?;
    }
    Ok(repo)
}

fn lock_text() -> Result<String, std::fmt::Error> {
    use std::fmt::Write as _;
    let mut binaries = String::new();
    for target in velnor_actions_contract_release::SUPPORTED_TARGETS {
        write!(
            &mut binaries,
            "[[generator.binaries]]\ntarget = \"{target}\"\nartifact = \"https://example.invalid/{target}\"\nsha256 = \"{}\"\n",
            "a".repeat(64)
        )?;
    }
    Ok(format!(
        "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{}\"\ncommit = \"{}\"\n{binaries}[mise-bootstrap]\nversion = \"2026.9.18\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n",
        env!("CARGO_PKG_VERSION"),
        "e".repeat(40),
        "b".repeat(64)
    ))
}

#[test]
fn output_export_fails_closed_when_the_pinned_helper_acquire_is_missing() -> TestResult {
    let repo = make_repo(&config_with_outputs("hosted"))?;
    let mut prep = prepare(repo.path())?;
    let job = prep
        .workflow
        .ir
        .jobs
        .get_mut("task-linux-lint")
        .expect("verification output job");
    job.steps.retain(|step| {
        step.role != Some(velnor_actions_contract_workflow::StepRole::AcquireVelnor)
    });

    let error = render_staged_tree(&prep).expect_err("missing staged helper fails closed");
    assert!(
        error
            .to_string()
            .contains("verification_job_contract:task-linux-lint"),
        "missing helper must fail job-contract validation: {error}"
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

fn config_with_outputs(mode: &str) -> String {
    config(mode).replacen(
        "timeout_minutes = 10\n[[workflow.tasks]]\nid = \"native-format\"",
        "timeout_minutes = 10\n[[workflow.tasks.outputs]]\nid = \"lint-result\"\npath = \"dist/lint-result.txt\"\nmax_bytes = 4096\n[[workflow.tasks]]\nid = \"native-format\"",
        1,
    )
}
