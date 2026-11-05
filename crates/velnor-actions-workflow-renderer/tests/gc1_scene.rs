// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::gc1_mbx_stub;
pub(crate) use crate::gc1_scene_key::{DEFAULT_SNAPSHOT, Key};
use crate::gc1_scene_resolve::resolve;
pub(crate) use crate::gc1_scene_tree::{Tree, files_under};
use crate::gc1_scene_tree::{tree_of, unlock_tree};

#[path = "gc1_scene_exec.rs"]
mod exec;

const RECOVERY_RECORD: &str = "velnor/state-recovery.record";
const PAYLOAD: &str = "velnor-gc1/transport/state/payload.tar.zst";

pub(crate) struct Rendered {
    pub(crate) run: String,
    pub(crate) env: Vec<(String, String)>,
}

pub(crate) struct Scripts {
    pub(crate) discover: String,
    pub(crate) recover: Rendered,
    pub(crate) export: String,
}

pub(crate) struct Scene {
    pub(crate) scratch: PathBuf,
    pub(crate) runner: PathBuf,
    pub(crate) workspace: PathBuf,
    bin: PathBuf,
    pub(crate) github_output: PathBuf,
    pub(crate) scripts: Scripts,
}

pub(crate) fn fail(message: &str) -> Box<dyn Error> {
    std::io::Error::other(message.to_owned()).into()
}

impl Scene {
    pub(crate) fn new(name: &str, scripts: Scripts) -> Result<Self, Box<dyn Error>> {
        Self::build(name, scripts, false)
    }

    pub(crate) fn runner_in_workspace(
        name: &str,
        scripts: Scripts,
    ) -> Result<Self, Box<dyn Error>> {
        Self::build(name, scripts, true)
    }

    fn build(name: &str, scripts: Scripts, nested: bool) -> Result<Self, Box<dyn Error>> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let scratch =
            std::env::temp_dir().join(format!("velnor-gc1-{}-{nanos}-{name}", std::process::id()));
        let workspace = scratch.join("workspace");
        let runner = if nested {
            workspace.join("runner")
        } else {
            scratch.join("runner")
        };
        let bin = scratch.join("bin");
        std::fs::create_dir_all(runner.join("velnor/mbx/actions"))?;
        std::fs::create_dir_all(&workspace)?;
        std::fs::write(workspace.join("Cargo.lock"), "lock-fixture")?;
        gc1_mbx_stub::install(&bin)?;
        Ok(Self {
            github_output: scratch.join("github-output"),
            scratch,
            runner,
            workspace,
            bin,
            scripts,
        })
    }

    pub(crate) fn payload(&self) -> PathBuf {
        self.runner.join(PAYLOAD)
    }

    pub(crate) fn stage(&self) -> PathBuf {
        self.runner.join("velnor/state-stage")
    }

    pub(crate) fn state_root(&self) -> PathBuf {
        self.runner.join("velnor-gc1/state")
    }

    pub(crate) fn recovery_record(&self) -> PathBuf {
        self.runner.join(RECOVERY_RECORD)
    }

    pub(crate) fn store(&self) -> PathBuf {
        self.runner.join("velnor/mbx")
    }

    pub(crate) fn store_files(&self) -> Result<Vec<PathBuf>, Box<dyn Error>> {
        files_under(&self.store())
    }

    pub(crate) fn mark_producer(&self) -> Result<(), Box<dyn Error>> {
        std::fs::write(self.runner.join("velnor/state-producer-ok"), "")?;
        Ok(())
    }

    pub(crate) fn recover(
        &self,
        matched: &str,
        env: &[(&str, &str)],
        fsize_blocks: Option<u64>,
    ) -> Result<String, Box<dyn Error>> {
        let bound = self.bind_to_payload(matched);
        self.recover_unbound(&bound, env, fsize_blocks)
    }

    pub(crate) fn recover_unbound(
        &self,
        matched: &str,
        env: &[(&str, &str)],
        fsize_blocks: Option<u64>,
    ) -> Result<String, Box<dyn Error>> {
        self.recover_selected(matched, matched, env, fsize_blocks)
    }

    pub(crate) fn recover_selected(
        &self,
        matched: &str,
        selected: &str,
        env: &[(&str, &str)],
        fsize_blocks: Option<u64>,
    ) -> Result<String, Box<dyn Error>> {
        self.fresh_outputs()?;
        let step = &self.scripts.recover;
        let run = resolve(&step.run, matched, selected, &self.runner)?;
        let mut resolved = Vec::new();
        for (name, value) in &step.env {
            resolved.push((
                name.clone(),
                resolve(value, matched, selected, &self.runner)?,
            ));
        }
        let mut pairs: Vec<(&str, &str)> = resolved
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        pairs.extend_from_slice(env);
        self.run_script(&run, &pairs, fsize_blocks)
    }

    fn fresh_outputs(&self) -> Result<(), Box<dyn Error>> {
        match std::fs::remove_file(&self.github_output) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }

    pub(crate) fn export(&self, env: &[(&str, &str)]) -> Result<String, Box<dyn Error>> {
        self.fresh_outputs()?;
        self.run_script(&self.scripts.export, env, None)
    }

    pub(crate) fn stub_export(
        &self,
        dir: &Path,
        env: &[(&str, &str)],
    ) -> Result<(), Box<dyn Error>> {
        let status = Command::new(self.bin.join("mbx"))
            .args([
                "cache",
                "export",
                "--group",
                "velnor",
                "--format",
                "directory",
            ])
            .arg(dir)
            .envs(env.iter().copied())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(fail("fixture export failed"))
        }
    }

    pub(crate) fn pack_export(&self, mark: &str) -> Result<(), Box<dyn Error>> {
        let parent = self.scratch.join("pack");
        std::fs::create_dir_all(self.payload().parent().ok_or_else(|| fail("no parent"))?)?;
        self.stub_export(
            &parent.join("state"),
            &[(gc1_mbx_stub::EXPORT_MARK_ENV, mark)],
        )?;
        let mut tar = Command::new("tar")
            .args(["-cf", "-", "-C"])
            .arg(&parent)
            .arg("state")
            .stdout(Stdio::piped())
            .spawn()?;
        let tar_out = tar
            .stdout
            .take()
            .ok_or_else(|| fail("tar stdout unavailable"))?;
        let compressed = Command::new("zstd")
            .args(["-3", "--long=30", "-q", "-f", "-o"])
            .arg(self.payload())
            .stdin(tar_out)
            .status()?;
        let packed = tar.wait()?;
        if packed.success() && compressed.success() {
            Ok(())
        } else {
            Err(fail("fixture pack failed"))
        }
    }

    pub(crate) fn scratch_dir(&self) -> PathBuf {
        self.runner.join("velnor/state-recover")
    }

    pub(crate) fn tree(&self) -> Result<Tree, Box<dyn Error>> {
        tree_of(&self.scratch)
    }

    pub(crate) fn install_shim(&self, tool: &str, body: &str) -> Result<(), Box<dyn Error>> {
        gc1_mbx_stub::write_executable(&self.bin.join(tool), body)
    }

    pub(crate) fn restricted_path(&self, commands: &[&str]) -> Result<String, Box<dyn Error>> {
        use std::os::unix::fs::symlink;

        let directory = self.scratch.join("restricted-bin");
        std::fs::create_dir_all(&directory)?;
        let paths: Vec<PathBuf> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
        for command in commands {
            let source = paths
                .iter()
                .map(|path| path.join(command))
                .find(|path| path.is_file())
                .ok_or_else(|| fail(&format!("cannot find fixture command {command}")))?;
            symlink(source, directory.join(command))?;
        }
        Ok(directory.display().to_string())
    }

    pub(crate) fn install_zstd_pad(&self) -> Result<(), Box<dyn Error>> {
        gc1_mbx_stub::install_zstd_pad(&self.bin)
    }

    pub(crate) fn shim_zstd_log(&self) -> Result<(), Box<dyn Error>> {
        gc1_mbx_stub::install_zstd_log(&self.bin)
    }

    pub(crate) fn shim_tar_log(&self) -> Result<(), Box<dyn Error>> {
        gc1_mbx_stub::install_tar_log(&self.bin)
    }

    pub(crate) fn shim_failure(&self, tool: &str) -> Result<(), Box<dyn Error>> {
        gc1_mbx_stub::install_failure(&self.bin, tool)
    }

    pub(crate) fn finish(self) -> Result<(), Box<dyn Error>> {
        unlock_tree(&self.scratch);
        std::fs::remove_dir_all(&self.scratch)?;
        Ok(())
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        unlock_tree(&self.scratch);
        drop(std::fs::remove_dir_all(&self.scratch));
    }
}
