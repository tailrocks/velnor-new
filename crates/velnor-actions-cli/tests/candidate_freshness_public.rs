// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

use tempfile::TempDir;
use velnor_actions_contract::{REQUIRED_JOB_ID, StepKind};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{finalized_jobs, prepare};

mod candidate_freshness_public_support;
use candidate_freshness_public_support::{status, write_mbx_transport, write_mise_transport};

const TASK_NAME: &str = "check-generated-workflow-freshness";
const TASK_JOB: &str = "task-generated-workflow-freshness";

#[test]
fn verification_task_checks_generated_workflow_content_without_repair() -> Result<(), Box<dyn Error>>
{
    run_freshness("import", FreshnessMutation::ChangeWorkflow)
}

#[test]
fn verification_task_checks_claude_symlink_identity_without_repair() -> Result<(), Box<dyn Error>> {
    run_freshness("symlink", FreshnessMutation::ReplaceLinkWithCopy)
}

#[test]
fn candidate_mode_requires_a_report_aware_trusted_helper() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let paths = FixturePaths::new(&temp);
    paths.create("import")?;
    let config = paths.repo.join(".velnor/config.toml");
    let bootstrap = fs::read_to_string(&config)?;
    fs::write(
        &config,
        bootstrap.replace(
            "generator_validation = \"bootstrap\"",
            "generator_validation = \"candidate\"",
        ),
    )?;
    let error = match prepare(&paths.repo) {
        Ok(_) => return Err("candidate mode accepted without a trusted report parser".into()),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("candidate_requires_report_aware_helper"),
        "unexpected Candidate mode rejection: {error}"
    );
    Ok(())
}

#[test]
fn verification_task_rejects_a_mise_lock_with_a_different_rust_profile()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let paths = FixturePaths::new(&temp);
    paths.create("import")?;
    let admitted = prepare(&paths.repo)?;
    let _ = finalized_jobs(&admitted)?;
    let lock_path = paths.repo.join(".velnor/mise.lock");
    let lock = fs::read_to_string(&lock_path)?;
    let changed = lock.replace("profile = \"minimal\"", "profile = \"default\"");
    assert_ne!(lock, changed, "fixture lock has no explicit Rust profile");
    fs::write(lock_path, changed)?;
    let error = match prepare(&paths.repo) {
        Ok(_) => return Err("task accepted a lock with a different Rust profile".into()),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("verification_tool_identity_or_lock"),
        "unexpected Rust profile lock rejection: {error}"
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum FreshnessMutation {
    ChangeWorkflow,
    ReplaceLinkWithCopy,
}

fn run_freshness(mode: &str, mutation: FreshnessMutation) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let paths = FixturePaths::new(&temp);
    paths.create(mode)?;
    let task = mise_task_run()?;
    let jobs = finalized_jobs(&prepare(&paths.repo)?)?;
    assert_task_job(&jobs)?;
    let cli = PathBuf::from(env!("CARGO_BIN_EXE_velnor-actions"));
    let initial = run_cli_generate(&cli, &paths)?;
    assert!(initial.success(), "fixture generation failed: {initial}");
    let passing = run_task(&task, &paths)?;
    assert!(
        passing.success(),
        "declared freshness task failed: {passing}"
    );
    let changed_path = match mutation {
        FreshnessMutation::ChangeWorkflow => paths.repo.join(".github/workflows/ci.yml"),
        FreshnessMutation::ReplaceLinkWithCopy => paths.repo.join(".github/CLAUDE.md"),
    };
    let before = fs::read(&changed_path)?;
    match mutation {
        FreshnessMutation::ChangeWorkflow => {
            let mut changed = before.clone();
            changed.extend_from_slice(b"\n# freshness mutation\n");
            fs::write(&changed_path, changed)?;
        }
        FreshnessMutation::ReplaceLinkWithCopy => {
            fs::remove_file(&changed_path)?;
            fs::write(&changed_path, &before)?;
        }
    }
    let mutated = fs::read(&changed_path)?;
    let rejected = run_task(&task, &paths)?;
    assert!(!rejected.success(), "mutated generated tree was accepted");
    assert_eq!(
        fs::read(&changed_path)?,
        mutated,
        "freshness task repaired input"
    );
    match mutation {
        FreshnessMutation::ChangeWorkflow => {
            assert_eq!(
                mutated,
                [before.as_slice(), b"\n# freshness mutation\n"].concat()
            );
        }
        FreshnessMutation::ReplaceLinkWithCopy => {
            assert_eq!(mutated, before);
            assert!(fs::metadata(&changed_path)?.is_file());
            assert!(
                !fs::symlink_metadata(&changed_path)?
                    .file_type()
                    .is_symlink()
            );
        }
    }
    assert!(
        !paths.preview_root().exists(),
        "task left its preview behind"
    );
    Ok(())
}

fn assert_task_job(
    jobs: &std::collections::BTreeMap<String, velnor_actions_contract::Job>,
) -> Result<(), Box<dyn Error>> {
    let task = jobs.get(TASK_JOB).ok_or("verification task job missing")?;
    assert_eq!(task.needs, Vec::<String>::new());
    assert!(
        jobs.get(REQUIRED_JOB_ID)
            .ok_or("Required job missing")?
            .needs
            .contains(&TASK_JOB.to_owned())
    );
    assert!(!jobs.contains_key("candidate"));
    let install = task
        .steps
        .iter()
        .find(|step| step.name == "Install declared prebuilt task tools")
        .ok_or("declared prebuilt task tool installation missing")?;
    let StepKind::Shell { run, .. } = &install.kind else {
        return Err("task tool installation was not a shell step".into());
    };
    let install_script = run.join(" ");
    assert!(
        install_script.contains("profile = \"minimal\""),
        "task did not preserve the locked Rust profile"
    );
    for (key, tool) in [
        ("rust", PinnedTool::Rust),
        ("mr-boxington", PinnedTool::MrBoxington),
        ("github:jdx/jactionlint", PinnedTool::Jactionlint),
        ("shellcheck", PinnedTool::Shellcheck),
    ] {
        assert!(
            install_script.contains(&format!("[tools.\"{key}\"]"))
                && install_script.contains(&format!(
                    "version = \"{}\"",
                    ToolCatalog::pinned().version(tool)
                )),
            "task omitted pinned tool {key}"
        );
    }
    let task_step = task
        .steps
        .iter()
        .find(|step| step.name == "Run declared Mise task")
        .ok_or("declared Mise task step missing")?;
    let StepKind::Shell { run, .. } = &task_step.kind else {
        return Err("declared Mise task was not a shell step".into());
    };
    assert!(run.join(" ").contains(&format!(
        "mise --no-env --locked --no-hooks run --skip-tools {TASK_NAME}"
    )));
    Ok(())
}

fn mise_task_run() -> Result<String, Box<dyn Error>> {
    let document: toml::Value = toml::from_str(include_str!("../../../.velnor/mise.toml"))?;
    let run = document
        .get("tasks")
        .and_then(|tasks| tasks.get(TASK_NAME))
        .and_then(|task| task.get("run"))
        .and_then(toml::Value::as_str)
        .ok_or("freshness Mise task body missing")?;
    Ok(run.to_owned())
}

struct FixturePaths {
    root: PathBuf,
    repo: PathBuf,
    bin: PathBuf,
    runner_temp: PathBuf,
}

impl FixturePaths {
    fn new(temp: &TempDir) -> Self {
        Self {
            root: temp.path().to_path_buf(),
            repo: temp.path().join("repo"),
            bin: temp.path().join("bin"),
            runner_temp: temp.path().join("runner-temp"),
        }
    }

    fn create(&self, mode: &str) -> Result<(), Box<dyn Error>> {
        fs::create_dir_all(self.repo.join(".velnor"))?;
        fs::create_dir_all(&self.bin)?;
        fs::create_dir_all(&self.runner_temp)?;
        fs::create_dir_all(self.root.join("home"))?;
        fs::write(self.repo.join(".velnor/config.toml"), config_text(mode))?;
        fs::write(
            self.repo.join(".velnor/generator.lock"),
            include_str!("../../../.velnor/generator.lock"),
        )?;
        fs::write(
            self.repo.join(".velnor/controller-approvals.json"),
            include_str!("../../../.velnor/controller-approvals.json"),
        )?;
        fs::write(
            self.repo.join(".velnor/mise.toml"),
            include_str!("../../../.velnor/mise.toml"),
        )?;
        fs::write(
            self.repo.join(".velnor/mise.lock"),
            include_str!("../../../.velnor/mise.lock"),
        )?;
        fs::write(
            self.repo.join(".velnor/rust-toolchain.toml"),
            include_str!("../../../rust-toolchain.toml"),
        )?;
        fs::write(
            self.repo.join("rust-toolchain.toml"),
            include_str!("../../../rust-toolchain.toml"),
        )?;
        write_mise_transport(&self.bin)?;
        write_mbx_transport(&self.bin)?;
        git(&self.repo, ["init", "-b", "main"], self)?;
        git(
            &self.repo,
            [
                "remote",
                "add",
                "origin",
                "https://github.com/tailrocks/velnor-new.git",
            ],
            self,
        )?;
        git(&self.repo, ["add", "."], self)?;
        git(
            &self.repo,
            [
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-m",
                "fixture",
            ],
            self,
        )?;
        Ok(())
    }

    fn preview_root(&self) -> PathBuf {
        self.runner_temp.join("velnor-actions-freshness-501-1")
    }
}

fn config_text(mode: &str) -> String {
    format!(
        "schema = 2\n\n[workflow]\n\
         policy = \"velnor-repository-v1\"\n\
         default_branch = \"main\"\n\
         generator_validation = \"bootstrap\"\n\n\
         [[workflow.tasks]]\n\
         kind = \"verification\"\n\
         id = \"generated-workflow-freshness\"\n\
         mise_task = \"{TASK_NAME}\"\n\
         source = {{ mise_config = \".velnor/mise.toml\", working_directory = \".velnor\" }}\n\
         runner = \"linux-x64\"\n\
         timeout_minutes = 30\n\n\
         [execution]\n\
         default_profile = \"hosted\"\n\
         hosted_profile = \"hosted\"\n\
         scale_set_profile = \"local\"\n\
         [execution.profiles.hosted]\n\
         kind = \"github-hosted\"\n\
         label = \"ubuntu-26.04\"\n\
         platform = \"linux/amd64\"\n\
         [execution.profiles.local]\n\
         kind = \"github-scale-set\"\n\
         name = \"ubuntu-26.04-scale-set\"\n\
         labels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\n\
         platform = \"linux/amd64\"\n\n\
         [agents]\n\
         claude_md = \"{mode}\"\n"
    )
}

fn run_cli_generate(cli: &Path, paths: &FixturePaths) -> Result<ExitStatus, Box<dyn Error>> {
    let mut command = Command::new(cli);
    command.arg("generate").current_dir(&paths.repo).env_clear();
    set_test_env(&mut command, paths);
    Ok(command
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("RUNNER_TEMP", &paths.runner_temp)
        .env("GITHUB_RUN_ID", "501")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("MISE_AUTO_INSTALL", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()?)
}

fn run_task(task: &str, paths: &FixturePaths) -> Result<ExitStatus, Box<dyn Error>> {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-e", "-c", task])
        .current_dir(paths.repo.join(".velnor"))
        .env_clear();
    set_test_env(&mut command, paths);
    Ok(command
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("RUNNER_TEMP", &paths.runner_temp)
        .env("GITHUB_RUN_ID", "501")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env(
            "VELNOR_ACTIONS_BINARY",
            env!("CARGO_BIN_EXE_velnor-actions"),
        )
        .env("MISE_AUTO_INSTALL", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()?)
}

fn git<const N: usize>(
    repo: &Path,
    args: [&str; N],
    paths: &FixturePaths,
) -> Result<(), Box<dyn Error>> {
    let mut command = Command::new("git");
    command.args(args).current_dir(repo).env_clear();
    set_test_env(&mut command, paths);
    status(
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .status()?,
        "Git fixture setup failed",
    )
}

fn set_test_env(command: &mut Command, paths: &FixturePaths) {
    let mut path = paths.bin.as_os_str().to_os_string();
    path.push(":");
    path.push(if cfg!(target_os = "macos") {
        "/usr/bin:/bin:/usr/sbin:/sbin"
    } else {
        "/usr/bin:/bin"
    });
    command
        .env("PATH", path)
        .env("HOME", paths.root.join("home"));
}
