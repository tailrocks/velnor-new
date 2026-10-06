//! Native workload generation and closed execution regressions.

#[path = "../../test_support/git_fixture.rs"]
mod git_fixture;

use std::fs;
use tempfile::TempDir;
use velnor_actions_contract::{Provenance, StepKind};
use velnor_actions_mise::ToolCatalog;

use crate::{GenerationPreparation, prepare, render_staged_tree};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture(
    kind: &str,
    settings: &str,
    files: &[&str],
) -> Result<TempDir, Box<dyn std::error::Error>> {
    let root = TempDir::new()?;
    let output = git_fixture::command(root.path())?
        .args(["init", "-b", "testmain"])
        .output()?;
    assert!(output.status.success(), "git init: {:?}", output.stderr);
    fs::create_dir_all(root.path().join(".velnor"))?;
    fs::write(
        root.path().join(".velnor/config.toml"),
        format!(
            "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[[stacks.workloads]]\nname = \"native\"\nkind = \"{kind}\"\n{settings}"
        ),
    )?;
    for file in files {
        let path = root.path().join(file);
        fs::create_dir_all(path.parent().ok_or("missing parent")?)?;
        fs::write(path, "fixture")?;
    }
    Ok(root)
}

fn workload_job(prep: &GenerationPreparation) -> (&String, &velnor_actions_contract::Job) {
    prep.workflow
        .ir
        .jobs
        .iter()
        .find(|(id, _)| id.starts_with("workload-"))
        .expect("substantive workload job")
}

fn shell_text(job: &velnor_actions_contract::Job) -> String {
    job.steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::Shell { run, .. } => Some(run.join(" ")),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn docker_only_required_depends_on_build_and_installs_no_rust() -> TestResult {
    let root = fixture("docker_build", "", &["Dockerfile"])?;
    fs::write(
        root.path().join("mise.toml"),
        "[tasks.test]\nrun = 'malicious-project-task'\n",
    )?;
    let prep = prepare(root.path())?;
    let (id, job) = workload_job(&prep);
    assert!(prep.workflow.ir.jobs["required"].needs.contains(id));
    assert_eq!(prep.discovery.proposals.len(), 1);
    let commands = shell_text(job);
    assert!(!commands.contains("docker build"), "{commands}");
    assert!(!commands.contains("rust@"), "{commands}");
    assert!(!commands.contains("malicious-project-task"), "{commands}");
    assert!(!commands.contains("mise run"), "{commands}");
    let build_ref = assert_docker_action_coverage(job, &prep.discovery.proposals[0].task_id)?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(".github/workflows/ci.yml").ok_or("missing CI")?;
    assert!(yaml.contains(&build_ref), "{yaml}");
    assert!(!yaml.contains("docker build"), "{yaml}");
    assert!(
        !yaml.contains("rust@"),
        "pure Docker workflow installs Rust: {yaml}"
    );
    Ok(())
}

fn assert_docker_action_coverage(
    job: &velnor_actions_contract::Job,
    task_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let build_ref = format!(
        "docker/build-push-action@{}",
        velnor_actions_actionlint::actions::BUILD_PUSH_ACTION_SHA
    );
    let actions: Vec<_> = job
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| {
            let StepKind::Action { uses, with, .. } = &step.kind else {
                return None;
            };
            (uses == &build_ref).then_some((index, step, with))
        })
        .collect();
    assert_eq!(actions.len(), 2, "validation and qualified cache export");
    let (build_index, build, inputs) = actions[0];
    assert_docker_preparation(job, build_index)?;
    assert_eq!(inputs["context"], ".");
    assert_eq!(inputs["file"], "Dockerfile");
    assert_eq!(inputs["load"], "true");
    assert_eq!(inputs["push"], "false");
    assert_eq!(inputs["github-token"], "");
    assert!(!inputs.contains_key("cache-to"));
    let (report_index, report_env) = job
        .steps
        .iter()
        .enumerate()
        .find_map(|(index, step)| {
            let StepKind::Shell { env, .. } = &step.kind else {
                return None;
            };
            (env.get("VELNOR_INTERNAL_OP").map(String::as_str) == Some(crate::ACTION_REPORT_OP))
                .then_some((index, env))
        })
        .ok_or("missing container obligation report")?;
    assert!(
        job.steps[report_index]
            .condition
            .as_deref()
            .ok_or("report gate")?
            .contains("always()")
    );
    assert_eq!(report_env["VELNOR_TASK_ID"], task_id);
    let action_id = build
        .id
        .as_ref()
        .ok_or("missing validation outcome binding")?;
    assert_eq!(
        report_env["VELNOR_ACTION_OUTCOME"],
        format!("${{{{ steps.{}.outcome }}}}", action_id.as_str())
    );
    let (export_index, export, inputs) = actions[1];
    assert!(build_index < report_index && report_index < export_index);
    assert_eq!(inputs["load"], "false");
    assert_eq!(inputs["outputs"], "type=cacheonly");
    assert!(inputs["cache-to"].contains("ignore-error=true"));
    assert!(!inputs.contains_key("tags"));
    let gate = export.condition.as_deref().ok_or("export gate")?;
    assert!(gate.contains(velnor_actions_contract::workflow::cache_trust::CACHE_SAVE_CONDITION));
    Ok(build_ref)
}

fn assert_docker_preparation(job: &velnor_actions_contract::Job, build_index: usize) -> TestResult {
    use velnor_actions_actionlint::actions::{
        BUILDKIT_IMAGE_DIGEST, BUILDX_VERSION, SETUP_BUILDX_ACTION_SHA,
    };
    let setup_ref = format!("docker/setup-buildx-action@{SETUP_BUILDX_ACTION_SHA}");
    let setup: Vec<_> = job
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| match &step.kind {
            StepKind::Action { uses, with, env } if uses == &setup_ref => {
                Some((index, step, with, env))
            }
            _ => None,
        })
        .collect();
    assert_eq!(setup.len(), 1, "one pinned isolated builder owner");
    let (index, step, inputs, env) = setup[0];
    assert!(index < build_index, "builder must precede validation");
    assert_eq!(step.name, "Prepare container builder");
    assert_eq!(step.condition, None);
    assert_eq!(inputs["version"], format!("v{BUILDX_VERSION}"));
    assert_eq!(inputs["driver"], "docker-container");
    assert_eq!(
        inputs["driver-opts"],
        format!("image=moby/buildkit@{BUILDKIT_IMAGE_DIGEST}")
    );
    assert_eq!(inputs["buildkitd-flags"], "--oci-worker-gc=true");
    assert_eq!(inputs["cache-binary"], "false");
    assert_eq!(inputs.len(), 5);
    assert_eq!(
        env["DOCKER_CONFIG"],
        "${{ runner.temp }}/velnor/native/docker/config"
    );
    assert_eq!(env["DOCKER_BUILD_RECORD_UPLOAD"], "false");
    assert_eq!(env["DOCKER_BUILD_SUMMARY"], "false");
    assert_eq!(env.len(), 3);
    assert!(
        !job.steps
            .iter()
            .any(|step| step.name == "Prepare native tools")
    );
    Ok(())
}

#[test]
fn bun_keeps_install_build_test_order_and_frozen_install() -> TestResult {
    let root = fixture(
        "bun_ci",
        "inputs = [\"bun.lock\"]\n",
        &["package.json", "bun.lock"],
    )?;
    let prep = prepare(root.path())?;
    let phases: std::collections::BTreeMap<_, _> = prep
        .discovery
        .proposals
        .iter()
        .map(|task| (task.task_kind.as_str(), task))
        .collect();
    assert_eq!(phases.len(), 3);
    assert!(phases["install"].depends_on.is_empty());
    assert_eq!(
        phases["build"].depends_on,
        [phases["install"].task_id.clone()]
    );
    assert_eq!(phases["test"].depends_on, [phases["build"].task_id.clone()]);
    let commands = shell_text(workload_job(&prep).1);
    let install = commands
        .find("bun install --frozen-lockfile")
        .ok_or("no frozen install")?;
    let build = commands.find("bun run build").ok_or("no build")?;
    let test = commands.find("bun run test").ok_or("no test")?;
    assert!(install < build && build < test, "{commands}");
    assert!(!commands.contains("rust@"), "{commands}");
    Ok(())
}

#[test]
fn swift_uses_macos_26_and_builds_before_tests() -> TestResult {
    let root = fixture("swift_test", "", &["Package.swift"])?;
    let prep = prepare(root.path())?;
    let (_, job) = workload_job(&prep);
    assert_eq!(job.runs_on, "macos-26");
    let commands = shell_text(job);
    let build = commands.find("swift build").ok_or("no Swift build")?;
    let test = commands
        .find("swift test --parallel")
        .ok_or("no Swift test")?;
    assert!(build < test, "{commands}");
    assert!(!commands.contains("rust@"), "{commands}");
    Ok(())
}

#[test]
fn ruby_multiple_files_produce_one_syntax_obligation() -> TestResult {
    let root = fixture(
        "ruby_syntax",
        "paths = [\"a.rb\", \"b.rb\"]\n",
        &["a.rb", "b.rb"],
    )?;
    let prep = prepare(root.path())?;
    assert_eq!(prep.discovery.proposals.len(), 1);
    let task = &prep.discovery.proposals[0];
    assert_eq!(task.task_kind, "syntax");
    let args: Vec<_> = task
        .payload
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect();
    assert!(args.iter().any(|arg| arg.contains("ARGV.each")));
    assert_eq!(&args[args.len() - 2..], ["a.rb", "b.rb"]);
    assert_eq!(
        prep.workflow
            .ir
            .jobs
            .keys()
            .filter(|id| id.starts_with("workload-"))
            .count(),
        1
    );
    Ok(())
}

#[test]
fn explicit_file_arguments_resolve_from_configured_workload_root() -> TestResult {
    for (kind, file) in [("ruby_syntax", "a.rb"), ("shellcheck", "check.sh")] {
        let path = format!("native/{file}");
        let settings = format!("root = \"native\"\npaths = [\"{path}\"]\n");
        let root = fixture(kind, &settings, &[&path])?;
        let prep = prepare(root.path())?;
        let task = &prep.discovery.proposals[0];
        let argument = task.payload.last().ok_or("no file argument")?;
        let resolved = root.path().join(&task.identity.project_root).join(argument);
        assert!(
            resolved.is_file(),
            "{kind} argument resolves to missing {}",
            resolved.display()
        );
    }
    Ok(())
}

#[test]
fn missing_workload_manifest_fails_before_emission() -> TestResult {
    for (kind, path) in [
        ("docker_build", "Dockerfile"),
        ("bun_ci", "package.json"),
        ("swift_test", "Package.swift"),
    ] {
        let root = fixture(kind, "", &[])?;
        let error = prepare(root.path()).expect_err("missing native manifest must fail");
        assert!(
            error
                .to_string()
                .contains(&format!("workload_evidence_missing:{path}")),
            "{error}"
        );
        assert!(!root.path().join(".github").exists());
    }
    Ok(())
}

#[test]
fn native_execution_unknown_forbids_reuse_and_argv_ignores_project_mise() -> TestResult {
    let root = fixture("bun_ci", "", &["package.json", "bun.lock"])?;
    let prep = prepare(root.path())?;
    for task in &prep.discovery.proposals {
        assert!(!task.cache_policy.allow_task_reuse);
        assert!(!task.cache_policy.allow_compilation_reuse);
        let closure = crate::internal_plan::workload_identity::closure(
            task,
            &Provenance::Known {
                digest: velnor_actions_contract::digest_b3(b"checkout"),
            },
            "graph",
            "tools",
            "platform",
        );
        assert!(closure.unknown_inputs().contains(&"native_execution"));
        let argv = super::argv(task, &ToolCatalog::pinned())?;
        assert!(argv.iter().any(|arg| arg == "--no-config"), "{argv:?}");
        assert!(argv.iter().any(|arg| arg == "--no-env"), "{argv:?}");
        assert!(argv.iter().any(|arg| arg == "--no-hooks"), "{argv:?}");
    }
    Ok(())
}

#[test]
fn bun_requires_a_lock_for_frozen_install() -> TestResult {
    let root = fixture("bun_ci", "", &["package.json"])?;
    let error = prepare(root.path()).expect_err("frozen install requires a committed lock");
    assert!(
        error.to_string().contains("workload_bun_lock_missing:."),
        "{error}"
    );
    assert!(!root.path().join(".github").exists());
    for lock in ["bun.lock", "bun.lockb"] {
        let locked = fixture("bun_ci", "", &["package.json", lock])?;
        assert_eq!(prepare(locked.path())?.discovery.proposals.len(), 3);
    }
    Ok(())
}

#[test]
fn undeclared_docker_evidence_cannot_generate_no_work_ci() -> TestResult {
    let root = fixture("docker_build", "", &["Dockerfile"])?;
    fs::write(
        root.path().join(".velnor/config.toml"),
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n",
    )?;
    let error = prepare(root.path()).expect_err("Docker evidence needs a substantive obligation");
    assert!(
        error
            .to_string()
            .contains("native_obligation_undeclared:Dockerfile"),
        "{error}"
    );
    assert!(!root.path().join(".github").exists());
    Ok(())
}
