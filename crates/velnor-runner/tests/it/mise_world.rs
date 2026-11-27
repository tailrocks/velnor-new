// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use crate::exec_support::{Ran, Scratch};
use crate::mise_store::StoreMode;

pub(crate) use crate::mise_helpers::{
    git, job_fixture, manifest, pinned_mise_action, real_action, sha256_hex,
};

const DEFAULT_SERVER: &str = "file:///velnor-test";
pub(crate) const VERSION: &str = "2026.11.10";
pub(crate) const SHA_PLACEHOLDER: &str = "__SETUP_VELNOR_SHA__";
pub(crate) const CONFIG: &str = "[settings]\nlockfile = true\n\n[tools]\nnode = \"26.13.1\"\n";
const LOCK: &str = "lockfile_version = 3\n\n[[tools.jactionlint]]\nversion = \"2.6.0\"\nbackend = \"github:jdx/jactionlint\"\n\n[[tools.shellcheck]]\nversion = \"0.11.0\"\nbackend = \"aqua:koalaman/shellcheck\"\n\n[[tools.node]]\nversion = \"26.13.1\"\nbackend = \"core:node\"\n";
const TOOL: &str = "#!/bin/sh\necho tooly-ran\n";

pub(crate) struct World {
    scratch: Scratch,
    pub(crate) store: PathBuf,
    pub(crate) root: PathBuf,
    temp: PathBuf,
    pub(crate) store_mode: StoreMode,
}

impl World {
    pub(crate) fn new() -> Result<Self> {
        let scratch = Scratch::new()?;
        let root = std::fs::canonicalize(scratch.workspace().with_file_name(""))
            .context("resolve the scratch root")?;
        let store = root.join("store");
        std::fs::create_dir_all(&store).context("create the store")?;
        let temp = root.join("temp");
        Ok(Self {
            scratch,
            store,
            root,
            temp,
            store_mode: StoreMode::Sealed,
        })
    }

    pub(crate) fn workspace(&self) -> PathBuf {
        self.scratch.workspace()
    }

    pub(crate) fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    pub(crate) fn temp(&self) -> PathBuf {
        self.temp.clone()
    }

    pub(crate) fn canonical_temp(&self) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.temp).context("create the temp directory")?;
        std::fs::canonicalize(&self.temp).context("resolve the temp directory")
    }

    pub(crate) fn symlinked_temp(&mut self) -> Result<()> {
        let real = self.root.join("tmp-real");
        let link = self.root.join("tmp-link");
        std::fs::create_dir_all(&real).context("create the real temp directory")?;
        std::os::unix::fs::symlink("tmp-real", &link).context("link the temp directory")?;
        self.temp = link;
        Ok(())
    }

    pub(crate) fn mirror(&self) -> PathBuf {
        crate::native_checkout_layout::mirror_in(&self.store, &self.server_url(), 1)
    }

    pub(crate) fn server_url(&self) -> String {
        if self.upstream().exists() {
            format!("file://{}", self.root.join("up").display())
        } else {
            DEFAULT_SERVER.to_owned()
        }
    }

    pub(crate) fn upstream(&self) -> PathBuf {
        self.root.join("up/tailrocks/velnor-new.git")
    }

    pub(crate) fn binary_dir(&self) -> PathBuf {
        self.store.join("tools/mise").join(VERSION)
    }

    pub(crate) fn install_mise(&self) -> Result<PathBuf> {
        self.unseal_store()?;
        let directory = self.binary_dir();
        std::fs::create_dir_all(directory.join("tool-bin")).context("create the tools store")?;
        let binary = directory.join("mise");
        std::fs::copy(manifest("tests/fixtures/mise-double/mise"), &binary)
            .context("copy the mise double")?;
        let tool = directory.join("tool-bin/tooly");
        std::fs::write(&tool, TOOL).context("write the tool")?;
        std::fs::set_permissions(&tool, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .context("make the tool executable")?;
        self.configure_double(&[])?;
        Ok(binary)
    }

    pub(crate) fn init_workspace(&self, commit_lock: bool) -> Result<()> {
        self.init_workspace_with(CONFIG, commit_lock)
    }

    pub(crate) fn init_workspace_with(&self, config: &str, commit_lock: bool) -> Result<()> {
        self.write_workspace(config.as_bytes(), LOCK.as_bytes(), commit_lock)
    }

    pub(crate) fn init_workspace_with_lock(&self, config: &str, lock: &str) -> Result<()> {
        self.write_workspace(config.as_bytes(), lock.as_bytes(), true)
    }

    pub(crate) fn write_workspace(
        &self,
        config: &[u8],
        lock: &[u8],
        commit_lock: bool,
    ) -> Result<()> {
        let workspace = self.workspace();
        std::fs::create_dir_all(&workspace).context("create the workspace")?;
        git(&workspace, &["init", "--quiet"])?;
        std::fs::write(workspace.join("mise.toml"), config).context("write mise.toml")?;
        std::fs::write(workspace.join("mise.lock"), lock).context("write mise.lock")?;
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
        self.unseal_store()?;
        let source = self.root.join("action-source");
        let directory = source.join("actions/setup-velnor");
        std::fs::create_dir_all(&directory).context("create the action source")?;
        std::fs::write(directory.join("action.yml"), action).context("write action.yml")?;
        git(&source, &["init", "--quiet", "-b", "main"])?;
        git(&source, &["add", "."])?;
        git(&source, &["commit", "--quiet", "--message", "action"])?;
        let upstream = self.upstream();
        std::fs::create_dir_all(&upstream).context("create the upstream")?;
        git(&upstream, &["init", "--quiet", "--bare"])?;
        git(
            &source,
            &["push", "--quiet", &upstream.to_string_lossy(), "main"],
        )?;
        let mirror = self.mirror();
        std::fs::create_dir_all(mirror.with_file_name("")).context("create the mirror parent")?;
        git(
            &source,
            &["clone", "--quiet", "--bare", ".", &mirror.to_string_lossy()],
        )?;
        git(&source, &["rev-parse", "HEAD"])
    }

    pub(crate) fn with_text(&self, job: &Path, replace: &[(&str, &str)]) -> Result<PathBuf> {
        let mut text = std::fs::read_to_string(job)
            .context("read the job fixture")?
            .replace(DEFAULT_SERVER, &self.server_url());
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

    pub(crate) fn with_final_condition(&self, job: &Path, condition: &str) -> Result<PathBuf> {
        let mut message: Value =
            serde_json::from_slice(&std::fs::read(job).context("read the job fixture")?)
                .context("parse the job fixture")?;
        message["Steps"][1]["Condition"] = condition.into();
        let patched = self.root.join("with-condition.json");
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
        self.unseal_store()?;
        let mut command = self.command(job)?;
        self.seal_store()?;
        self.declare_read_only(&mut command);
        command
            .env("VELNOR_STORE_ROOT", &self.store)
            .env_remove("MISE_TRUSTED_CONFIG_PATHS")
            .env_remove("MISE_YES")
            .env_remove("CARGO_HOME")
            .env_remove("RUSTUP_HOME")
            .env("HOME", self.home())
            .envs(extra_env.iter().copied());
        command.spawn().context("start velnor-runner exec")
    }

    pub(crate) fn command(&self, job: &Path) -> Result<Command> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-runner"));
        command
            .arg("exec")
            .arg("--job")
            .arg(job)
            .arg("--workspace")
            .arg(self.workspace())
            .arg("--temp")
            .arg(&self.temp)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(facts) = self.trusted_facts()? {
            command.arg("--trusted").arg(facts);
        }
        Ok(command)
    }

    pub(crate) fn run(&self, job: &Path, extra_env: &[(&str, &str)]) -> Result<Ran> {
        let child = self.start(job, extra_env)?;
        Self::finish(child)
    }

    pub(crate) fn finish(child: Child) -> Result<Ran> {
        let started = Instant::now();
        let output = child.wait_with_output().context("run velnor-runner exec")?;
        Ok(Ran {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            code: output.status.code(),
            elapsed: started.elapsed(),
        })
    }
}

pub(crate) fn assert_failed(ran: &Ran, expected: &str) {
    assert!(ran.has_line(expected), "{expected}\n{}", ran.stdout);
    assert!(ran.has_line("step 1 failed"), "{}", ran.stdout);
    assert!(ran.has_line("job failed"), "{}", ran.stdout);
    assert_eq!(ran.code, Some(1));
}
