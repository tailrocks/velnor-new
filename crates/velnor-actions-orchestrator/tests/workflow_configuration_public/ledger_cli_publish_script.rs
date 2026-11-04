// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::ledger_cli_publish_step;

use super::workflow_configuration_fixture;

const RUNNER_DEFAULT_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

fn canonical(path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    Ok(fs::canonicalize(path)?)
}

struct Publish {
    target: TempDir,
    runner_temp: TempDir,
    workspace: TempDir,
    outputs: PathBuf,
}

impl Publish {
    fn new(binary: &str) -> Result<Self, Box<dyn Error>> {
        let workspace = tempfile::tempdir()?;
        workflow_configuration_fixture::git(&["init", "-b", "testmain"], workspace.path())?;
        fs::write(workspace.path().join("file"), "x")?;
        workflow_configuration_fixture::git(&["add", "file"], workspace.path())?;
        workflow_configuration_fixture::git(&["commit", "-m", "fixture"], workspace.path())?;
        let runner_temp = tempfile::tempdir()?;
        let target = tempfile::tempdir()?;
        let debug = target.path().join("debug");
        fs::create_dir_all(&debug)?;
        let installed = debug.join("velnor-actions");
        fs::write(&installed, binary)?;
        fs::set_permissions(&installed, fs::Permissions::from_mode(0o755))?;
        let outputs = runner_temp.path().join("github-output");
        Ok(Self {
            target,
            runner_temp,
            workspace,
            outputs,
        })
    }

    fn run(&self) -> Result<bool, Box<dyn Error>> {
        let step = ledger_cli_publish_step()?;
        let StepKind::Shell { run, env } = &step.kind else {
            return Err("publish is not a shell step".into());
        };
        Ok(Command::new(&run[0])
            .args(&run[1..])
            .envs(env)
            .env("PATH", RUNNER_DEFAULT_PATH)
            .env("CARGO_TARGET_DIR", canonical(self.target.path())?)
            .env("RUNNER_TEMP", canonical(self.runner_temp.path())?)
            .env("GITHUB_OUTPUT", &self.outputs)
            .current_dir(self.workspace.path())
            .status()?
            .success())
    }

    fn output_lines(&self) -> String {
        fs::read_to_string(&self.outputs).unwrap_or_default()
    }
}

const HELPER_BUILD_BINARY: &str = "#!/bin/sh\necho 'velnor-actions: this ledger helper build only runs emit-group-ledger-v1 and assemble-group-ledgers-v1 through VELNOR_INTERNAL_OP' >&2\nexit 2\n";
const FULL_BUILD_BINARY: &str = "#!/bin/sh\necho 'Velnor Actions: generate GitHub Actions workflows from repository state' >&2\nexit 2\n";
const SILENT_BINARY: &str = "#!/bin/sh\nexit 0\n";
const WRONG_CODE_BINARY: &str = "#!/bin/sh\necho 'velnor-actions: this ledger helper build only runs emit-group-ledger-v1' >&2\nexit 1\n";

#[test]
fn publish_uploads_a_binary_that_rejects_the_smoke_probe() -> Result<(), Box<dyn Error>> {
    let publish = Publish::new(HELPER_BUILD_BINARY)?;
    assert!(publish.run()?);
    let lines = publish.output_lines();
    assert!(lines.contains("sha256="), "{lines}");
    assert!(lines.contains("source_sha="), "{lines}");
    Ok(())
}

#[test]
fn publish_refuses_a_binary_that_is_not_the_helper_build() -> Result<(), Box<dyn Error>> {
    for binary in [FULL_BUILD_BINARY, SILENT_BINARY, WRONG_CODE_BINARY] {
        let publish = Publish::new(binary)?;
        assert!(!publish.run()?, "{binary}");
        assert_eq!(publish.output_lines(), "", "{binary}");
    }
    Ok(())
}
