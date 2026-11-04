// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;
use velnor_actions_contract::{Step, StepKind};
use velnor_actions_workflow_renderer::state_seal::skeleton::{
    state_skeleton_restore_step, state_skeleton_step,
};

pub(super) const HASH: &str = "0123456789abcdef";
pub(super) const OTHER_HASH: &str = "fedcba9876543210";
pub(super) const LANES: [&str; 3] = [
    "target/debug",
    "target/velnor-clippy/debug",
    "target/velnor-doc/debug",
];
const MISE_SHIM: &str = "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--\" ]; then shift; break; fi\n  shift\ndone\nif [ \"${1-}\" = cargo ] && [ -n \"${CARGO-}\" ]; then shift; exec \"$CARGO\" \"$@\"; fi\nexec \"$@\"\n";

pub(super) struct Rig {
    _workspace_dir: TempDir,
    pub(super) workspace: PathBuf,
    pub(super) temp: TempDir,
    tools: TempDir,
}

impl Rig {
    pub(super) fn new() -> Result<Self, Box<dyn Error>> {
        let workspace_dir = tempfile::tempdir()?;
        let workspace = workspace_dir.path().canonicalize()?;
        let tools = tempfile::tempdir()?;
        let shim = tools.path().join("mise");
        fs::write(&shim, MISE_SHIM)?;
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755))?;
        let rig = Self {
            _workspace_dir: workspace_dir,
            workspace,
            temp: tempfile::tempdir()?,
            tools,
        };
        rig.write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"foo\", \"my-crate\", \"my-tool\"]\nresolver = \"2\"\n",
        )?;
        for (name, extra) in [("foo", ""), ("my-crate", "")] {
            rig.write(
                &format!("{name}/Cargo.toml"),
                &format!(
                    "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{extra}"
                ),
            )?;
            rig.write(&format!("{name}/src/lib.rs"), "pub fn f() {}\n")?;
            rig.write(&format!("{name}/build.rs"), "fn main() {}\n")?;
        }
        rig.write("my-crate/src/main.rs", "fn main() {}\n")?;
        rig.write(
            "my-tool/Cargo.toml",
            "[package]\nname = \"my-tool\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )?;
        rig.write("my-tool/src/main.rs", "fn main() {}\n")?;
        rig.write("my-tool/build.rs", "fn main() {}\n")?;
        rig.write("foo/tests/it.rs", "#[test]\nfn it() {}\n")?;
        rig.write(".gitignore", "target\n")?;
        let lock = Command::new(cargo())
            .args(["generate-lockfile", "--offline"])
            .current_dir(&rig.workspace)
            .env_remove("CARGO_TARGET_DIR")
            .output()?;
        assert!(
            lock.status.success(),
            "{}",
            String::from_utf8_lossy(&lock.stderr)
        );
        rig.git(&["init", "-b", "testmain"])?;
        rig.git(&["add", "."])?;
        rig.git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "seed",
        ])?;
        Ok(rig)
    }

    pub(super) fn git(&self, args: &[&str]) -> Result<(), Box<dyn Error>> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.workspace)
            .output()?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
        }
        Ok(())
    }

    pub(super) fn write(&self, relative: &str, content: &str) -> Result<(), Box<dyn Error>> {
        let path = self.workspace.join(relative);
        fs::create_dir_all(path.parent().ok_or("path has no parent")?)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub(super) fn store(&self) -> PathBuf {
        self.temp.path().join("velnor/mbx")
    }

    pub(super) fn manifests(&self) -> PathBuf {
        self.store().join("actions/task-manifests")
    }

    pub(super) fn seed_predecessor(&self) -> Result<(), Box<dyn Error>> {
        let manifests = self.manifests().join("v1");
        fs::create_dir_all(&manifests)?;
        fs::write(manifests.join("project.json"), "predecessor")?;
        fs::create_dir_all(self.store().join("actions/cas"))?;
        Ok(())
    }

    pub(super) fn simulate_phase_one(&self) -> Result<(), Box<dyn Error>> {
        let manifests = self.manifests().join("v1");
        fs::create_dir_all(&manifests)?;
        fs::write(manifests.join("project.json"), "stub-inputs")?;
        fs::write(manifests.join("stub-only.json"), "stub-inputs")?;
        fs::create_dir_all(self.store().join("incremental"))?;
        fs::write(self.store().join("incremental/state"), "x")?;
        Ok(())
    }

    pub(super) fn touch(&self, relative: &str) -> Result<(), Box<dyn Error>> {
        self.write(relative, "x")
    }

    pub(super) fn exists(&self, relative: &str) -> bool {
        self.workspace.join(relative).exists()
    }

    pub(super) fn run(&self, step: &Step) -> Result<Output, Box<dyn Error>> {
        let StepKind::Shell { run, env } = &step.kind else {
            return Err("step left its shell form".into());
        };
        let (program, args) = run.split_first().ok_or("step has no command")?;
        let path = format!(
            "{}:{}",
            self.tools.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(&self.workspace)
            .envs(env)
            .env("PATH", path)
            .env("RUNNER_TEMP", self.temp.path())
            .env("GITHUB_WORKSPACE", &self.workspace)
            .env("GITHUB_ENV", self.temp.path().join("github_env"))
            .env("MBX_CACHE_DIR", self.store())
            .env("GITHUB_RUN_ID", "7")
            .env("VELNOR_RUN_KEY_ATTEMPT", "1")
            .env_remove("CARGO_TARGET_DIR");
        Ok(command.output()?)
    }

    pub(super) fn stub(&self) -> Result<(), Box<dyn Error>> {
        let step = state_skeleton_step("1.99.1", ".", BTreeMap::new())?;
        let output = self.run(&step)?;
        assert!(
            output.status.success(),
            "stub failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }

    pub(super) fn restore(&self) -> Result<Output, Box<dyn Error>> {
        self.run(&state_skeleton_restore_step()?)
    }

    pub(super) fn seed_member_and_registry_outputs(&self) -> Result<(), Box<dyn Error>> {
        self.seed_member_and_registry_outputs_in("target")
    }

    pub(super) fn seed_member_and_registry_outputs_in(
        &self,
        target: &str,
    ) -> Result<(), Box<dyn Error>> {
        for lane in LANES {
            let lane = lane.replacen("target", target, 1);
            for name in [
                format!("libfoo-{HASH}.rlib"),
                format!("libfoo-{HASH}.rmeta"),
                format!("foo-{HASH}.d"),
                format!("it-{HASH}"),
                format!("it-{HASH}.d"),
                format!("libmy_crate-{HASH}.rlib"),
                format!("my_crate-{HASH}.d"),
                format!("my_crate-{HASH}"),
                format!("my_tool-{HASH}"),
                format!("my_tool-{HASH}.d"),
            ] {
                self.touch(&format!("{lane}/deps/{name}"))?;
            }
            for name in [
                format!("libserde-{HASH}.rlib"),
                format!("serde-{HASH}.d"),
                format!("libfoo_bar-{HASH}.rlib"),
                format!("foo_bar-{HASH}.d"),
                format!("libfoo-{HASH}.d"),
                format!("build_script_build-{HASH}"),
                format!("foo-{HASH}x"),
                format!("my_tool_extra-{HASH}.d"),
            ] {
                self.touch(&format!("{lane}/deps/{name}"))?;
            }
            for dir in [
                format!("foo-{HASH}"),
                format!("foo-{OTHER_HASH}"),
                format!("my-crate-{HASH}"),
                format!("my-tool-{HASH}"),
            ] {
                self.touch(&format!("{lane}/build/{dir}/out/generated.rs"))?;
            }
            for dir in [
                format!("foo-bar-{HASH}"),
                format!("serde-{HASH}"),
                format!("foo-extra-{HASH}"),
                format!("my-tool-extra-{HASH}"),
            ] {
                self.touch(&format!("{lane}/build/{dir}/output"))?;
            }
        }
        Ok(())
    }
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

pub(super) fn read(path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(path)?)
}
