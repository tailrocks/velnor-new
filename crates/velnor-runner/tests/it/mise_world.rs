// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::exec_support::{Ran, Scratch};

pub(crate) const VERSION: &str = "2026.11.10";
pub(crate) const SHA_PLACEHOLDER: &str = "__SETUP_VELNOR_SHA__";
pub(crate) const CONFIG: &str = "[settings]\nlockfile = true\n\n[tools]\nnode = \"26.13.1\"\n";
const LOCK: &str = "lockfile_version = 3\n\n[[tools.jactionlint]]\nversion = \"2.6.0\"\n\n[[tools.shellcheck]]\nversion = \"0.11.0\"\n\n[[tools.node]]\nversion = \"26.13.1\"\n";
const TOOL: &str = "#!/bin/sh\necho tooly-ran\n";

pub(crate) struct World {
    scratch: Scratch,
    pub(crate) store: PathBuf,
    root: PathBuf,
}

pub(crate) fn pinned_mise_action() -> Result<String> {
    let policy: toml::Table = include_str!("../../../../.velnor/version-policy.toml")
        .parse()
        .context("parse the version policy")?;
    policy["actions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|action| action["name"].as_str() == Some("jdx/mise-action"))
        .and_then(|action| action["sha"].as_str())
        .map(str::to_owned)
        .context("jdx/mise-action is not pinned in the version policy")
}

pub(crate) fn real_action() -> Result<String> {
    let path = manifest("../../actions/setup-velnor/action.yml");
    std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))
}

pub(crate) fn manifest(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

pub(crate) fn job_fixture(name: &str) -> PathBuf {
    manifest(&format!("tests/fixtures/jobs/{name}.json"))
}

pub(crate) fn git(directory: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .with_context(|| format!("run git {args:?}"))?;
    ensure!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(crate) fn sha256_hex(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        }))
}

impl World {
    pub(crate) fn new() -> Result<Self> {
        let scratch = Scratch::new()?;
        let root = scratch.workspace().with_file_name("");
        let store = root.join("store");
        std::fs::create_dir_all(&store).context("create the store")?;
        Ok(Self {
            scratch,
            store,
            root,
        })
    }

    pub(crate) fn workspace(&self) -> PathBuf {
        self.scratch.workspace()
    }

    pub(crate) fn temp(&self) -> PathBuf {
        self.root.join("temp")
    }

    pub(crate) fn mirror(&self) -> PathBuf {
        self.store.join("git/_local/tailrocks/velnor-new.git")
    }

    pub(crate) fn binary_dir(&self) -> PathBuf {
        self.store.join("tools/mise").join(VERSION)
    }

    fn log(&self) -> PathBuf {
        self.root.join("mise.log")
    }

    pub(crate) fn install_mise(&self) -> Result<PathBuf> {
        let directory = self.binary_dir();
        std::fs::create_dir_all(directory.join("tool-bin")).context("create the tools store")?;
        let binary = directory.join("mise");
        std::fs::copy(manifest("tests/fixtures/mise-double/mise"), &binary)
            .context("copy the mise double")?;
        let tool = directory.join("tool-bin/tooly");
        std::fs::write(&tool, TOOL).context("write the tool")?;
        std::fs::set_permissions(&tool, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .context("make the tool executable")?;
        Ok(binary)
    }

    pub(crate) fn init_workspace(&self, commit_lock: bool) -> Result<()> {
        self.init_workspace_with(CONFIG, commit_lock)
    }

    pub(crate) fn init_workspace_with(&self, config: &str, commit_lock: bool) -> Result<()> {
        let workspace = self.workspace();
        std::fs::create_dir_all(&workspace).context("create the workspace")?;
        git(&workspace, &["init", "--quiet"])?;
        std::fs::write(workspace.join("mise.toml"), config).context("write mise.toml")?;
        std::fs::write(workspace.join("mise.lock"), LOCK).context("write mise.lock")?;
        let tracked: &[&str] = if commit_lock {
            &["mise.toml", "mise.lock"]
        } else {
            &["mise.toml"]
        };
        git(&workspace, &[&["add"], tracked].concat())?;
        git(&workspace, &["commit", "--quiet", "--message", "tools"])?;
        Ok(())
    }

    pub(crate) fn publish_action(&self, action: &str) -> Result<String> {
        let source = self.root.join("action-source");
        let directory = source.join("actions/setup-velnor");
        std::fs::create_dir_all(&directory).context("create the action source")?;
        std::fs::write(directory.join("action.yml"), action).context("write action.yml")?;
        git(&source, &["init", "--quiet"])?;
        git(&source, &["add", "."])?;
        git(&source, &["commit", "--quiet", "--message", "action"])?;
        let mirror = self.mirror();
        std::fs::create_dir_all(mirror.with_file_name("")).context("create the mirror parent")?;
        git(
            &source,
            &["clone", "--quiet", "--bare", ".", &mirror.to_string_lossy()],
        )?;
        git(&source, &["rev-parse", "HEAD"])
    }

    pub(crate) fn with_text(&self, job: &Path, replace: &[(&str, &str)]) -> Result<PathBuf> {
        let mut text = std::fs::read_to_string(job).context("read the job fixture")?;
        for (from, to) in replace {
            ensure!(text.contains(from), "{from} is not in {}", job.display());
            text = text.replace(from, to);
        }
        let patched = self.root.join("patched.json");
        std::fs::write(&patched, text).context("write the job")?;
        Ok(patched)
    }

    pub(crate) fn without_input(&self, job: &Path, name: &str) -> Result<PathBuf> {
        let mut message: Value =
            serde_json::from_slice(&std::fs::read(job).context("read the job fixture")?)
                .context("parse the job fixture")?;
        let inputs = message["Steps"][0]["Inputs"]["map"]
            .as_array_mut()
            .context("the step inputs")?;
        let before = inputs.len();
        inputs.retain(|pair| pair["key"]["lit"] != name);
        ensure!(inputs.len() < before, "{name} is not an input of the step");
        let patched = self.root.join("without-input.json");
        std::fs::write(&patched, serde_json::to_vec(&message)?).context("write the job")?;
        Ok(patched)
    }

    pub(crate) fn with_input(&self, job: &Path, name: &str, value: &str) -> Result<PathBuf> {
        let mut message: Value =
            serde_json::from_slice(&std::fs::read(job).context("read the job fixture")?)
                .context("parse the job fixture")?;
        let inputs = message["Steps"][0]["Inputs"]["map"]
            .as_array_mut()
            .context("the step inputs")?;
        inputs.retain(|pair| pair["key"]["lit"] != name);
        inputs.push(json!({
            "key": {"type": 0, "lit": name},
            "value": {"type": 0, "lit": value}
        }));
        let patched = self.root.join("with-input.json");
        std::fs::write(&patched, serde_json::to_vec(&message)?).context("write the job")?;
        Ok(patched)
    }

    pub(crate) fn with_job_env(&self, job: &Path, env: &[(&str, &str)]) -> Result<PathBuf> {
        let mut message: Value =
            serde_json::from_slice(&std::fs::read(job).context("read the job fixture")?)
                .context("parse the job fixture")?;
        let pairs: Vec<Value> = env
            .iter()
            .map(|(name, value)| {
                json!({
                    "key": {"type": 0, "lit": name},
                    "value": {"type": 0, "lit": value}
                })
            })
            .collect();
        message["EnvironmentVariables"] = json!([{"type": 2, "map": pairs}]);
        let patched = self.root.join("with-env.json");
        std::fs::write(&patched, serde_json::to_vec(&message)?).context("write the job")?;
        Ok(patched)
    }

    pub(crate) fn only_first_step(&self, job: &Path) -> Result<PathBuf> {
        let mut message: Value =
            serde_json::from_slice(&std::fs::read(job).context("read the job fixture")?)
                .context("parse the job fixture")?;
        message["Steps"]
            .as_array_mut()
            .context("the job steps")?
            .truncate(1);
        let patched = self.root.join("first-step.json");
        std::fs::write(&patched, serde_json::to_vec(&message)?).context("write the job")?;
        Ok(patched)
    }

    pub(crate) fn with_final_script(&self, job: &Path, script: &str) -> Result<PathBuf> {
        let mut message: Value =
            serde_json::from_slice(&std::fs::read(job).context("read the job fixture")?)
                .context("parse the job fixture")?;
        let inputs = message["Steps"][1]["Inputs"]["map"]
            .as_array_mut()
            .context("the script inputs")?;
        inputs[0]["value"]["lit"] = script.into();
        let patched = self.root.join("with-script.json");
        std::fs::write(&patched, serde_json::to_vec(&message)?).context("write the job")?;
        Ok(patched)
    }

    pub(crate) fn with_leading_script(&self, job: &Path, script: &str) -> Result<PathBuf> {
        let mut message: Value =
            serde_json::from_slice(&std::fs::read(job).context("read the job fixture")?)
                .context("parse the job fixture")?;
        let step = json!({
            "Type": "action",
            "Id": "00000000-0000-0000-0000-0000000000f1",
            "Name": "__run",
            "ContextName": "__run",
            "Reference": {"Type": "script"},
            "DisplayNameToken": {"type": 0, "lit": "Plant a fake git"},
            "Inputs": {"type": 2, "map": [
                {"key": {"type": 0, "lit": "script"}, "value": {"type": 0, "lit": script}}
            ]}
        });
        message["Steps"]
            .as_array_mut()
            .context("the job steps")?
            .insert(0, step);
        let patched = self.root.join("leading.json");
        std::fs::write(&patched, serde_json::to_vec(&message)?).context("write the job")?;
        Ok(patched)
    }

    pub(crate) fn fake_git_marker(&self) -> PathBuf {
        self.temp().join("fake-git-ran")
    }

    pub(crate) fn plant_fake_git(&self, job: &Path) -> Result<PathBuf> {
        let directory = self.temp().join("fake-bin");
        let script = format!(
            "mkdir -p {dir}\nprintf '#!/bin/sh\\necho ran >> {marker}\\nexit 1\\n' > {dir}/git\nchmod +x {dir}/git\necho {dir} >> \"$GITHUB_PATH\"",
            dir = directory.display(),
            marker = self.fake_git_marker().display()
        );
        self.with_leading_script(job, &script)
    }

    pub(crate) fn start(&self, job: &Path, extra_env: &[(&str, &str)]) -> Result<Child> {
        Command::new(env!("CARGO_BIN_EXE_velnor-runner"))
            .arg("exec")
            .arg("--job")
            .arg(job)
            .arg("--workspace")
            .arg(self.workspace())
            .arg("--temp")
            .arg(self.temp())
            .env("VELNOR_STORE_ROOT", &self.store)
            .env("MISE_DOUBLE_LOG", self.log())
            .env_remove("MISE_TRUSTED_CONFIG_PATHS")
            .env_remove("MISE_YES")
            .envs(extra_env.iter().copied())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("start velnor-runner exec")
    }

    pub(crate) fn run(&self, job: &Path, extra_env: &[(&str, &str)]) -> Result<Ran> {
        let started = Instant::now();
        let output = self
            .start(job, extra_env)?
            .wait_with_output()
            .context("run velnor-runner exec")?;
        Ok(Ran {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            code: output.status.code(),
            elapsed: started.elapsed(),
        })
    }

    pub(crate) fn mise_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.log())
            .map_or_default(|text| text.lines().map(str::to_owned).collect())
    }

    pub(crate) fn mise_installs(&self) -> Vec<String> {
        let mut calls = self.mise_calls();
        calls.retain(|call| call.starts_with("argv=install "));
        calls
    }
}

pub(crate) fn assert_failed(ran: &Ran, expected: &str) {
    assert!(ran.has_line(expected), "{expected}\n{}", ran.stdout);
    assert!(ran.has_line("step 1 failed"), "{}", ran.stdout);
    assert!(ran.has_line("job failed"), "{}", ran.stdout);
    assert_eq!(ran.code, Some(1));
}
