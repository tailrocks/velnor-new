use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

use tempfile::TempDir;
use velnor_actions_contract::{StepKind, StepRole};
use velnor_actions_orchestrator::{finalized_jobs, prepare};
mod candidate_freshness_public_support;
use candidate_freshness_public_support::{
    assert_candidate_tools, assert_generated_claude_link, assert_regular_file, assert_unchanged,
    expected_mutation, read_generated_workflow, reset_freshness_preview, write_mise_transport,
};

#[test]
fn import_freshness_uses_candidate_binary_and_detects_content_changes() -> Result<(), Box<dyn Error>>
{
    run_freshness("import", FreshnessMutation::ChangeWorkflow)
}

#[test]
fn symlink_freshness_rejects_byte_identical_regular_file_without_repair()
-> Result<(), Box<dyn Error>> {
    run_freshness("symlink", FreshnessMutation::ReplaceLinkWithCopy)
}

#[test]
fn candidate_rejects_tool_pins_that_do_not_match_its_verifiers() -> Result<(), Box<dyn Error>> {
    for (tool, version) in [("rust", "1.98.0"), ("jactionlint", "2.2.0")] {
        assert_candidate_pin_rejected(tool, version)?;
    }
    Ok(())
}

fn assert_candidate_pin_rejected(tool: &str, version: &str) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let paths = FixturePaths::new(&temp);
    paths.create("import")?;
    let config = paths.repo.join(".velnor/config.toml");
    let original = fs::read_to_string(&config)?;
    fs::write(
        config,
        format!("{original}\n[tools]\n{tool} = \"{version}\"\n"),
    )?;
    let error = match prepare(&paths.repo) {
        Ok(_) => return Err(format!("candidate accepted mismatched {tool} pin").into()),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains(&format!("candidate_tool_catalog_mismatch:{tool}")),
        "unexpected candidate pin error: {error}"
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
    let cli = PathBuf::from(env!("CARGO_BIN_EXE_velnor-actions"));
    let candidate = paths.runner_temp.join("velnor/candidate/velnor-actions");
    fs::create_dir_all(candidate.parent().ok_or("candidate parent missing")?)?;
    std::os::unix::fs::symlink(&cli, &candidate)?;
    let (freshness, env) = emitted_candidate_freshness(&paths.repo)?;
    let cli_status = run_cli_generate(&cli, &paths)?;
    assert!(
        cli_status.success(),
        "native CLI generation failed: {cli_status}"
    );
    let initial = run_emitted_step(&freshness, &env, &paths)?;
    assert!(
        initial.success(),
        "emitted freshness check failed: {initial}"
    );
    let generated_workflow = read_generated_workflow(&paths.runner_temp)?;
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
    assert_eq!(
        mutated,
        expected_mutation(
            matches!(mutation, FreshnessMutation::ChangeWorkflow),
            &before
        )
    );
    reset_freshness_preview(&paths.runner_temp)?;
    let rejected = run_emitted_step(&freshness, &env, &paths)?;
    assert!(!rejected.success(), "mutated generated tree was accepted");
    assert_eq!(
        read_generated_workflow(&paths.runner_temp)?,
        generated_workflow,
        "regeneration changed canonical workflow output"
    );
    assert_unchanged(&changed_path, &mutated)?;
    if matches!(mutation, FreshnessMutation::ReplaceLinkWithCopy) {
        assert_regular_file(&changed_path)?;
        assert_generated_claude_link(&paths.runner_temp)?;
    }
    Ok(())
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
        write_mise_transport(&self.bin)?;
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
}

fn config_text(mode: &str) -> String {
    format!(
        "schema = 2\n\n[workflow]\n\
         policy = \"velnor-repository-v1\"\n\
         default_branch = \"main\"\n\
         generator_validation = \"candidate\"\n\n\
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

fn git<const N: usize>(
    repo: &Path,
    args: [&str; N],
    paths: &FixturePaths,
) -> Result<(), Box<dyn Error>> {
    let mut command = Command::new("git");
    command.args(args).current_dir(repo).env_clear();
    set_test_env(&mut command, paths);
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0");
    status(command.status()?, "Git fixture setup failed")
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

type ShellCommand = (Vec<String>, BTreeMap<String, String>);

fn emitted_candidate_freshness(repo: &Path) -> Result<ShellCommand, Box<dyn Error>> {
    let jobs = finalized_jobs(&prepare(repo)?)?;
    let candidate = jobs.get("candidate").ok_or("candidate job missing")?;
    assert!(candidate.needs.iter().any(|need| need == "plan"));
    assert!(!jobs.contains_key("release"));
    assert!(!candidate.steps.iter().any(|step| matches!(
        &step.kind,
        StepKind::Internal { operation, .. } if operation == "plan-v1"
    )));
    assert!(
        jobs.values()
            .any(|job| job.needs.iter().any(|need| need == "candidate"))
    );
    assert_candidate_tools(candidate)?;
    let step = candidate
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::CheckGenerated))
        .ok_or("candidate freshness step missing")?;
    match &step.kind {
        StepKind::Shell { run, env } => {
            assert!(run.last().is_some_and(|script| {
                script.contains("$RUNNER_TEMP/velnor/candidate/velnor-actions")
            }));
            Ok((run.clone(), env.clone()))
        }
        _ => Err("candidate freshness step is not a shell step".into()),
    }
}

fn run_emitted_step(
    run: &[String],
    env: &BTreeMap<String, String>,
    paths: &FixturePaths,
) -> Result<ExitStatus, Box<dyn Error>> {
    let (program, args) = run.split_first().ok_or("freshness argv missing")?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(&paths.repo)
        .env_clear()
        .envs(env);
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

fn status(status: ExitStatus, message: &str) -> Result<(), Box<dyn Error>> {
    if status.success() {
        Ok(())
    } else {
        Err(format!("{message}: {status}").into())
    }
}
