// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::render_staged_tree;

use super::ledger_cli_verify_script::{
    BYTES, BYTES_SHA256, Planted, RenderedVerify, TAMPERED, rendered_step, resolve_on,
};
use super::tools_producers_donors::prepared_for;

const RESTORE_ACTION: &str = ".github/actions/velnor-tools-cache-restore/action.yml";
const SCRUB_ACTION: &str = ".github/actions/velnor-tools-cache-scrub/action.yml";
const ADMISSION_STEP: &str = "Keep only an exact cache hit or validated seed on a miss";
const REVERIFY_STEP: &str = "Re-verify restored Mise installs";
const SYSTEM_PATH: &str = "/usr/bin:/bin";
pub(super) const RUNNER_BASH: &str = "bash --noprofile --norc -e -o pipefail {0}";

pub(super) type Env = Vec<(String, String)>;

pub(super) fn pair(name: &str, value: &str) -> (String, String) {
    (name.to_owned(), value.to_owned())
}

pub(super) struct Sandbox {
    pub(super) home: TempDir,
    pub(super) runner_temp: TempDir,
    workspace: TempDir,
}

impl Sandbox {
    pub(super) fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            home: tempfile::tempdir()?,
            runner_temp: tempfile::tempdir()?,
            workspace: tempfile::tempdir()?,
        })
    }

    fn mise_root(&self) -> PathBuf {
        self.home.path().join(".local/share/mise")
    }

    fn base_env(&self) -> Env {
        vec![
            pair("HOME", &self.home.path().display().to_string()),
            pair(
                "RUNNER_TEMP",
                &self.runner_temp.path().display().to_string(),
            ),
            pair(
                "GITHUB_WORKSPACE",
                &self.workspace.path().display().to_string(),
            ),
        ]
    }

    fn run(
        &self,
        step: &RenderedVerify,
        extra: &[(String, String)],
    ) -> Result<Option<i32>, Box<dyn Error>> {
        Ok(self
            .spawn(&expand_shell(&step.shell), &step.run, extra)?
            .code)
    }

    pub(super) fn run_with_output(
        &self,
        shell: &str,
        run: &str,
        extra: &[(String, String)],
    ) -> Result<Captured, Box<dyn Error>> {
        let output = self.workspace.path().join("github_output");
        fs::write(&output, "")?;
        let mut env = vec![pair("GITHUB_OUTPUT", &output.display().to_string())];
        env.extend_from_slice(extra);
        let mut captured = self.spawn(shell, run, &env)?;
        captured.github_output = fs::read_to_string(&output)?;
        Ok(captured)
    }

    fn spawn(
        &self,
        shell: &str,
        run: &str,
        extra: &[(String, String)],
    ) -> Result<Captured, Box<dyn Error>> {
        let script = self.workspace.path().join("step.sh");
        fs::write(&script, run)?;
        let mut words = shell.split_whitespace();
        let program = resolve_on(words.next().ok_or("empty shell")?, SYSTEM_PATH)?;
        let output = Command::new(program)
            .args(words.map(|word| word.replace("{0}", &script.display().to_string())))
            .env_clear()
            .env("PATH", SYSTEM_PATH)
            .envs(self.base_env())
            .envs(extra.iter().map(|(name, value)| (name, value)))
            .current_dir(self.workspace.path())
            .output()?;
        Ok(Captured {
            code: output.status.code(),
            stdout: String::from_utf8(output.stdout)?,
            github_output: String::new(),
        })
    }
}

pub(super) struct Captured {
    pub(super) code: Option<i32>,
    pub(super) stdout: String,
    pub(super) github_output: String,
}

pub(super) fn expand_shell(shell: &str) -> String {
    if shell == "bash" {
        RUNNER_BASH.to_owned()
    } else {
        shell.to_owned()
    }
}

pub(super) struct Attack {
    planted: Planted,
    hook: PathBuf,
}

impl Attack {
    pub(super) fn new() -> Result<Self, Box<dyn Error>> {
        let planted = Planted::new(&[])?;
        let hook = planted.hook()?;
        Ok(Self { planted, hook })
    }

    pub(super) fn forging(payload: &str) -> Result<Self, Box<dyn Error>> {
        let planted = Planted::new(&[])?;
        let hook = planted.forging_hook(payload)?;
        Ok(Self { planted, hook })
    }

    pub(super) fn startup_env(&self) -> Env {
        let hook = self.hook.display().to_string();
        vec![pair("BASH_ENV", &hook), pair("ENV", &hook)]
    }

    pub(super) fn function_env(&self, digest: &str) -> Env {
        let function = format!(
            "() {{ echo hook >> \"{}\"; echo {digest}  forged; }}",
            self.planted.log().display()
        );
        ["sha256sum", "shasum"]
            .into_iter()
            .flat_map(|tool| {
                [
                    pair(&format!("BASH_FUNC_{tool}%%"), &function),
                    pair(tool, &function),
                ]
            })
            .collect()
    }

    pub(super) fn calls(&self) -> String {
        self.planted.calls()
    }
}

pub(super) fn step_in(action: &str, name: &str) -> Result<RenderedVerify, Box<dyn Error>> {
    let (_repo, prepared) = prepared_for(WorkflowPolicy::VelnorRepositoryV1)?;
    let tree = render_staged_tree(&prepared)?;
    rendered_step(tree.get(action).ok_or("action is missing")?, name)
}

fn admission_exit(
    step: &RenderedVerify,
    binary: &[u8],
    extra: &[(String, String)],
) -> Result<Option<i32>, Box<dyn Error>> {
    let runner = Sandbox::new()?;
    let bin = runner.mise_root().join("bin");
    fs::create_dir_all(&bin)?;
    fs::write(bin.join("mise"), binary)?;
    let key = format!("mise-tools-v2-{}", "a".repeat(64));
    let mut env = vec![
        pair("TOOLS_CACHE_HIT", "true"),
        pair("TOOLS_EXPECTED_KEY", &key),
        pair("TOOLS_MATCHED_KEY", &key),
        pair("TOOLS_SEED_ADMITTED", "false"),
        pair("TOOLS_MISE_SHA256", BYTES_SHA256),
    ];
    env.extend_from_slice(extra);
    runner.run(step, &env)
}

#[test]
fn admission_ignores_env_hooks_and_exported_functions() -> Result<(), Box<dyn Error>> {
    for action in [RESTORE_ACTION, SCRUB_ACTION] {
        let step = step_in(action, ADMISSION_STEP)?;
        assert_eq!(admission_exit(&step, BYTES, &[])?, Some(0), "{action}");
        assert_eq!(admission_exit(&step, TAMPERED, &[])?, Some(1), "{action}");
        let attack = Attack::new()?;
        let mut env = attack.startup_env();
        env.extend(attack.function_env(BYTES_SHA256));
        let exit = admission_exit(&step, TAMPERED, &env)?;
        assert_eq!((exit, attack.calls().as_str()), (Some(1), ""), "{action}");
    }
    Ok(())
}

fn install_outcome(
    step: &RenderedVerify,
    install: &[u8],
    extra: &[(String, String)],
) -> Result<(Option<i32>, bool), Box<dyn Error>> {
    let runner = Sandbox::new()?;
    let installs = runner.mise_root().join("installs");
    let archives = installs.join(".velnor-verified-archives");
    let version = installs.join("fake/1.0.0");
    fs::create_dir_all(&archives)?;
    fs::create_dir_all(&version)?;
    fs::write(archives.join(BYTES_SHA256), BYTES)?;
    fs::write(version.join("file"), install)?;
    let pin = format!("fake 1.0.0 linux-x64 https://example.invalid/fake.bin {BYTES_SHA256}");
    let mut env = vec![
        pair("VELNOR_TOOL_PINS", &pin),
        pair("RUNNER_OS", "Linux"),
        pair("RUNNER_ARCH", "X64"),
    ];
    env.extend_from_slice(extra);
    let exit = runner.run(step, &env)?;
    Ok((exit, version.exists()))
}

#[test]
fn reverify_discards_a_tampered_install_despite_env_hooks() -> Result<(), Box<dyn Error>> {
    let step = step_in(RESTORE_ACTION, REVERIFY_STEP)?;
    assert_eq!(install_outcome(&step, BYTES, &[])?, (Some(0), true));
    assert_eq!(install_outcome(&step, TAMPERED, &[])?, (Some(0), false));
    let attack = Attack::new()?;
    let hooked = attack.startup_env();
    let outcome = install_outcome(&step, TAMPERED, &hooked)?;
    assert_eq!((outcome, attack.calls().as_str()), ((Some(0), false), ""));
    let outcome = install_outcome(&step, BYTES, &hooked)?;
    assert_eq!((outcome, attack.calls().as_str()), ((Some(0), true), ""));
    Ok(())
}
