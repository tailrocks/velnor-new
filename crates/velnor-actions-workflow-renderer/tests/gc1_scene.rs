// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_KEY_EXPR;

use crate::gc1_mbx_stub;

const MATCHED_KEY_BAKE: &str = "${{steps.mbx-bundle-restore.outputs.cache-matched-key}}";
const RECOVERY_RECORD: &str = "velnor/state-recovery.record";
const PAYLOAD: &str = "velnor-gc1/transport/state/payload.tar.gz";

pub(crate) struct Scripts {
    pub(crate) discover: String,
    pub(crate) recover: String,
    pub(crate) export: String,
    pub(crate) record: String,
}

pub(crate) struct Key {
    pub(crate) key: String,
    pub(crate) restore_prefix: String,
}

pub(crate) struct Scene {
    pub(crate) scratch: PathBuf,
    pub(crate) runner: PathBuf,
    pub(crate) workspace: PathBuf,
    bin: PathBuf,
    github_output: PathBuf,
    scripts: Scripts,
}

pub(crate) fn fail(message: &str) -> Box<dyn Error> {
    std::io::Error::other(message.to_owned()).into()
}

pub(crate) fn files_under(root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else {
                found.push(entry.path());
            }
        }
    }
    Ok(found)
}

impl Scene {
    pub(crate) fn new(name: &str, scripts: Scripts) -> Result<Self, Box<dyn Error>> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let scratch =
            std::env::temp_dir().join(format!("velnor-gc1-{}-{nanos}-{name}", std::process::id()));
        let runner = scratch.join("runner");
        let workspace = scratch.join("workspace");
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

    pub(crate) fn run_script(
        &self,
        script: &str,
        env: &[(&str, &str)],
        fsize_blocks: Option<u64>,
    ) -> Result<String, Box<dyn Error>> {
        let path = format!("{}:{}", self.bin.display(), std::env::var("PATH")?);
        let mut command = Command::new("sh");
        match fsize_blocks {
            Some(blocks) => command
                .arg("-c")
                .arg("ulimit -f \"$1\"; exec sh -c \"$2\"")
                .arg("sh")
                .arg(blocks.to_string())
                .arg(script),
            None => command.arg("-c").arg(script),
        };
        command
            .current_dir(&self.workspace)
            .env("PATH", path)
            .env("RUNNER_TEMP", &self.runner)
            .env("GITHUB_WORKSPACE", &self.workspace)
            .env("GITHUB_OUTPUT", &self.github_output)
            .env("GITHUB_EVENT_NAME", "push")
            .env("GITHUB_REF", "refs/heads/main")
            .env("GITHUB_REF_PROTECTED", "true")
            .env("VELNOR_DEFAULT_REF", "refs/heads/main")
            .env("VELNOR_MBX_BUNDLE_VERSION", "1.2.3")
            .env("VELNOR_MBX_BUNDLE_TOOLCHAIN", "1.89.0")
            .env("VELNOR_MBX_BUNDLE_ROOT", ".")
            .env("MBX_SHARE_OUT_DIR", "1")
            .env("MBX_BUILD_SCRIPT_EXECUTION", "0")
            .env("MBX_CC", "0")
            .env("ImageOS", "ubuntu26")
            .env("MBX_CACHE_EXPORT_GROUP", "velnor")
            .env("MBX_CACHE_DIR", self.store());
        for (name, value) in env {
            command.env(name, value);
        }
        let output = command.output()?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if output.status.success() {
            Ok(text)
        } else {
            Err(fail(&format!("script failed the step: {text}")))
        }
    }

    pub(crate) fn output_value(&self, key: &str) -> Result<String, Box<dyn Error>> {
        let text = std::fs::read_to_string(&self.github_output)?;
        text.lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .map(str::to_owned)
            .ok_or_else(|| fail(&format!("github output lost: {key}")))
    }

    pub(crate) fn discover(&self) -> Result<Key, Box<dyn Error>> {
        self.run_script(&self.scripts.discover, &[], None)?;
        if self.output_value("provenance")? != "trusted" {
            return Err(fail("discover misclassified a trusted push"));
        }
        Ok(Key {
            key: self.output_value("key")?,
            restore_prefix: self.output_value("restore_keys")?,
        })
    }

    pub(crate) fn mark_producer(&self) -> Result<(), Box<dyn Error>> {
        std::fs::write(self.runner.join("velnor/state-producer-ok"), "")?;
        Ok(())
    }

    pub(crate) fn recover(
        &self,
        key: &str,
        matched: &str,
        env: &[(&str, &str)],
        fsize_blocks: Option<u64>,
    ) -> Result<String, Box<dyn Error>> {
        let script = self
            .scripts
            .recover
            .replace(MBX_BUNDLE_KEY_EXPR, key)
            .replace(MATCHED_KEY_BAKE, matched);
        self.run_script(&script, env, fsize_blocks)
    }

    pub(crate) fn export(&self, env: &[(&str, &str)]) -> Result<String, Box<dyn Error>> {
        self.run_script(&self.scripts.export, env, None)
    }

    pub(crate) fn record(&self) -> Result<String, Box<dyn Error>> {
        self.run_script(&self.scripts.record, &[], None)
    }

    pub(crate) fn pack_export(&self, mark: &str) -> Result<(), Box<dyn Error>> {
        let parent = self.scratch.join("pack");
        std::fs::create_dir_all(self.payload().parent().ok_or_else(|| fail("no parent"))?)?;
        let exported = Command::new(self.bin.join("mbx"))
            .args([
                "cache",
                "export",
                "--group",
                "velnor",
                "--format",
                "directory",
            ])
            .arg(parent.join("state"))
            .env(gc1_mbx_stub::EXPORT_MARK_ENV, mark)
            .status()?;
        let packed = Command::new("tar")
            .arg("-czf")
            .arg(self.payload())
            .arg("-C")
            .arg(&parent)
            .arg("state")
            .status()?;
        if exported.success() && packed.success() {
            Ok(())
        } else {
            Err(fail("fixture pack failed"))
        }
    }

    pub(crate) fn assert_cold(&self, output: &str, token: &str) -> Result<(), Box<dyn Error>> {
        for needle in ["::warning::cache_corrupt", token] {
            if !output.contains(needle) {
                return Err(fail(&format!("recover output lost {needle}: {output}")));
            }
        }
        for (path, what) in [
            (self.recovery_record(), "recovery record"),
            (self.state_root(), "state root"),
            (self.stage(), "stage"),
            (self.payload(), "payload"),
        ] {
            if path.exists() {
                return Err(fail(&format!("refused archive left the {what}")));
            }
        }
        if !self.store_files()?.is_empty() {
            return Err(fail("refused archive reached the store"));
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<(), Box<dyn Error>> {
        std::fs::remove_dir_all(&self.scratch)?;
        Ok(())
    }
}
