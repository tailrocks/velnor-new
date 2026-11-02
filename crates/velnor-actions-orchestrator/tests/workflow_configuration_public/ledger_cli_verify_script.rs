// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use velnor_actions_contract::{StepKind, WorkflowPolicy};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::ledger_cli_verify_step;
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::one_job_per_crate::mbx_repo_for;
use super::workflow_configuration_fixture;

const RUNNER_DEFAULT_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
const RUNNER_DEFAULT_SHELL: &str = "bash -e {0}";

const BYTES: &[u8] = b"velnor ledger cli fixture\n";
const BYTES_SHA256: &str = "6d57cceb8ae05f9a71cc4a0efa99f8940bb72dcd713b56fc89be7ed6e411cb63";
const TAMPERED: &[u8] = b"velnor ledger cli fixture!\n";
const OTHER_SHA256: &str = "1b8e733ab0bf5f7b7e7836a65a2a0fa6d36cdfb8de75e3ac3ab211f29a431453";
const WRONG_SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Checkout {
    runner_temp: TempDir,
    workspace: TempDir,
    head: String,
}

impl Checkout {
    fn new(contents: Option<&[u8]>) -> Result<Self, Box<dyn Error>> {
        let workspace = tempfile::tempdir()?;
        workflow_configuration_fixture::git(&["init", "-b", "testmain"], workspace.path())?;
        fs::write(workspace.path().join("file"), "x")?;
        workflow_configuration_fixture::git(&["add", "file"], workspace.path())?;
        workflow_configuration_fixture::git(&["commit", "-m", "fixture"], workspace.path())?;
        let head = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(workspace.path())
                .output()?
                .stdout,
        )?
        .trim()
        .to_owned();
        let runner_temp = tempfile::tempdir()?;
        let checkout = Self {
            runner_temp,
            workspace,
            head,
        };
        if let Some(contents) = contents {
            fs::create_dir_all(checkout.binary().parent().ok_or("no parent")?)?;
            fs::write(checkout.binary(), contents)?;
            fs::set_permissions(checkout.binary(), fs::Permissions::from_mode(0o644))?;
        }
        Ok(checkout)
    }

    fn binary(&self) -> PathBuf {
        self.runner_temp
            .path()
            .join("velnor/ledger-cli/velnor-actions")
    }

    fn mode(&self) -> Result<u32, Box<dyn Error>> {
        Ok(fs::metadata(self.binary())?.permissions().mode() & 0o777)
    }

    fn verify(&self, sha256: &str, source_sha: &str) -> Result<bool, Box<dyn Error>> {
        let step = ledger_cli_verify_step()?;
        let StepKind::Shell { run, env } = &step.kind else {
            return Err("verify is not a shell step".into());
        };
        let mut command = Command::new(&run[0]);
        command
            .args(&run[1..])
            .envs(env)
            .env("PATH", "/usr/bin:/bin:/sbin")
            .env("RUNNER_TEMP", canonical(self.runner_temp.path())?)
            .env("VELNOR_LEDGER_CLI_SHA256", sha256)
            .env("VELNOR_LEDGER_CLI_SOURCE_SHA", source_sha)
            .current_dir(self.workspace.path());
        Ok(command.status()?.success())
    }
}

fn canonical(path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    Ok(fs::canonicalize(path)?)
}

#[test]
fn matching_digest_and_source_make_the_binary_executable() -> Result<(), Box<dyn Error>> {
    let checkout = Checkout::new(Some(BYTES))?;
    assert!(checkout.verify(BYTES_SHA256, &checkout.head)?);
    assert_eq!(checkout.mode()? & 0o111, 0o111);
    Ok(())
}

#[test]
fn tampered_bytes_fail_and_stay_non_executable() -> Result<(), Box<dyn Error>> {
    let checkout = Checkout::new(Some(TAMPERED))?;
    assert!(!checkout.verify(BYTES_SHA256, &checkout.head)?);
    assert_eq!(checkout.mode()?, 0o644);
    Ok(())
}

#[test]
fn wrong_digest_fails_and_stays_non_executable() -> Result<(), Box<dyn Error>> {
    let checkout = Checkout::new(Some(BYTES))?;
    assert!(!checkout.verify(OTHER_SHA256, &checkout.head)?);
    assert_eq!(checkout.mode()?, 0o644);
    Ok(())
}

#[test]
fn wrong_source_sha_fails_and_stays_non_executable() -> Result<(), Box<dyn Error>> {
    let checkout = Checkout::new(Some(BYTES))?;
    assert!(!checkout.verify(BYTES_SHA256, WRONG_SOURCE)?);
    assert_eq!(checkout.mode()?, 0o644);
    Ok(())
}

#[test]
fn empty_expected_values_fail() -> Result<(), Box<dyn Error>> {
    let checkout = Checkout::new(Some(BYTES))?;
    assert!(!checkout.verify("", &checkout.head)?);
    assert!(!checkout.verify(BYTES_SHA256, "")?);
    assert_eq!(checkout.mode()?, 0o644);
    Ok(())
}

#[test]
fn missing_binary_fails() -> Result<(), Box<dyn Error>> {
    let checkout = Checkout::new(None)?;
    assert!(!checkout.verify(BYTES_SHA256, &checkout.head)?);
    Ok(())
}

struct RenderedVerify {
    shell: String,
    run: String,
}

fn scalar(value: &str) -> Result<String, Box<dyn Error>> {
    if value.starts_with('"') {
        Ok(serde_json::from_str(value)?)
    } else {
        Ok(value.to_owned())
    }
}

fn rendered_verify() -> Result<RenderedVerify, Box<dyn Error>> {
    let repo = mbx_repo_for(false, WorkflowPolicy::VelnorRepositoryV1)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let workflow = tree.get(WORKFLOW_PATH).ok_or("no ci workflow")?;
    let mut lines = workflow
        .lines()
        .skip_while(|line| !line.contains("name: Verify ledger CLI"));
    lines.next().ok_or("no verify step")?;
    let (mut shell, mut run) = (None, None);
    for line in lines.take_while(|line| !line.starts_with("      - ")) {
        let line = line.trim();
        shell = line
            .strip_prefix("shell: ")
            .map(scalar)
            .transpose()?
            .or(shell);
        run = line.strip_prefix("run: ").map(scalar).transpose()?.or(run);
    }
    Ok(RenderedVerify {
        shell: shell.unwrap_or_else(|| RUNNER_DEFAULT_SHELL.to_owned()),
        run: run.ok_or("verify step has no run")?,
    })
}

struct Planted {
    bin: TempDir,
    logs: TempDir,
}

impl Planted {
    fn new(tools: &[(&str, &str)]) -> Result<Self, Box<dyn Error>> {
        let planted = Self {
            bin: tempfile::tempdir()?,
            logs: tempfile::tempdir()?,
        };
        for (name, behaviour) in tools {
            let script = format!(
                "#!/bin/sh\necho {name} >> \"{}\"\n{behaviour}\n",
                planted.log().display()
            );
            let path = planted.bin.path().join(name);
            fs::write(&path, script)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        }
        Ok(planted)
    }

    fn log(&self) -> PathBuf {
        self.logs.path().join("calls")
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.log()).unwrap_or_default()
    }
}

fn resolve_on(program: &str, path: &str) -> Result<PathBuf, Box<dyn Error>> {
    if program.starts_with('/') {
        return Ok(PathBuf::from(program));
    }
    path.split(':')
        .map(|dir| Path::new(dir).join(program))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| format!("{program} not found on {path}").into())
}

impl Checkout {
    fn verify_rendered(
        &self,
        step: &RenderedVerify,
        planted: &Planted,
        sha256: &str,
    ) -> Result<bool, Box<dyn Error>> {
        let path = format!("{}:{RUNNER_DEFAULT_PATH}", planted.bin.path().display());
        let script = self.runner_temp.path().join("step.sh");
        fs::write(&script, &step.run)?;
        let mut words = step.shell.split_whitespace();
        let program = resolve_on(words.next().ok_or("empty shell")?, &path)?;
        let status = Command::new(program)
            .args(words.map(|word| word.replace("{0}", &script.display().to_string())))
            .env_clear()
            .env("PATH", &path)
            .env("RUNNER_TEMP", canonical(self.runner_temp.path())?)
            .env("VELNOR_LEDGER_CLI_SHA256", sha256)
            .env("VELNOR_LEDGER_CLI_SOURCE_SHA", &self.head)
            .current_dir(self.workspace.path())
            .status()?;
        Ok(status.success())
    }
}

#[test]
fn planted_bash_cannot_verify_or_run() -> Result<(), Box<dyn Error>> {
    let step = rendered_verify()?;
    let planted = Planted::new(&[("bash", "exit 0"), ("sh", "exit 0")])?;
    let checkout = Checkout::new(Some(TAMPERED))?;
    assert!(!checkout.verify_rendered(&step, &planted, BYTES_SHA256)?);
    assert_eq!(checkout.mode()?, 0o644);
    assert_eq!(planted.calls(), "");
    Ok(())
}

#[test]
fn planted_tools_cannot_forge_a_verified_binary() -> Result<(), Box<dyn Error>> {
    let step = rendered_verify()?;
    let checkout = Checkout::new(Some(TAMPERED))?;
    let digest = format!("echo {BYTES_SHA256}  forged");
    let head = format!("echo {}", checkout.head);
    let planted = Planted::new(&[
        ("sha256sum", digest.as_str()),
        ("shasum", digest.as_str()),
        ("git", head.as_str()),
        ("chmod", "exit 0"),
    ])?;
    assert!(!checkout.verify_rendered(&step, &planted, BYTES_SHA256)?);
    assert_eq!(checkout.mode()?, 0o644);
    assert_eq!(planted.calls(), "");
    Ok(())
}

#[test]
fn hostile_failing_tools_do_not_break_a_genuine_binary() -> Result<(), Box<dyn Error>> {
    let step = rendered_verify()?;
    let planted = Planted::new(&[
        ("sha256sum", "exit 1"),
        ("shasum", "exit 1"),
        ("git", "exit 1"),
        ("chmod", "exit 1"),
    ])?;
    let checkout = Checkout::new(Some(BYTES))?;
    assert!(checkout.verify_rendered(&step, &planted, BYTES_SHA256)?);
    assert_eq!(checkout.mode()? & 0o111, 0o111);
    assert_eq!(planted.calls(), "");
    Ok(())
}
