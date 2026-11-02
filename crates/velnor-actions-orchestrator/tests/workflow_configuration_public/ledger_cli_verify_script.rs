// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::ledger_cli_verify_step;

use super::workflow_configuration_fixture;

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
