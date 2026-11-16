// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use super::tools_cache_verify_hook::{Sandbox, pair};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Fixture {
    pub schema: u32,
    pub bun: PathBuf,
    pub mise: PathBuf,
    pub mise_sha256: String,
    pub mise_version: String,
    pub host: String,
    pub home: PathBuf,
    pub cache: PathBuf,
    pub archives: Vec<Archive>,
    #[serde(skip)]
    replacements: String,
    #[serde(skip)]
    metrics_url: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Archive {
    pub tool: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
}

impl Fixture {
    pub(super) fn load() -> Result<(Self, Replay), Box<dyn Error>> {
        let path = std::env::var_os("VELNOR_NONRUST_TOOLS_FIXTURE").ok_or("VELNOR_NONRUST_TOOLS_FIXTURE missing; use the isolated Bun provisioning recipe in docs/implemented/ci-maintenance/repo-verification.md")?;
        let mut fixture: Self =
            serde_json::from_slice(&fs::read(Path::new(&path).join("fixture.json"))?)?;
        assert_eq!(fixture.schema, 1);
        let target = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") => velnor_actions_contract::ReleaseTarget::LinuxX86_64,
            ("macos", "aarch64") => velnor_actions_contract::ReleaseTarget::MacosArm64,
            ("macos", "x86_64") => velnor_actions_contract::ReleaseTarget::MacosX86_64,
            _ => return Err("unsupported fixture host".into()),
        };
        let authority = velnor_actions_mise::catalog::current_mise_binary(target);
        assert_eq!(fixture.mise_sha256, authority.sha256);
        assert_eq!(fs::metadata(&fixture.mise)?.len(), authority.bytes);
        assert_eq!(
            fixture.mise_version,
            velnor_actions_mise::catalog::MISE_VERSION
        );
        let actual: String = Sha256::digest(fs::read(&fixture.mise)?)
            .iter()
            .flat_map(|byte| {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                [
                    char::from(HEX[usize::from(byte >> 4)]),
                    char::from(HEX[usize::from(byte & 15)]),
                ]
            })
            .collect();
        assert_eq!(actual, fixture.mise_sha256);
        assert!(
            !fixture
                .home
                .join(".local/share/mise/installs/.velnor-verified-archives")
                .exists()
        );
        assert_eq!(fixture.archives.len(), 2);
        let (replay, transport) = Replay::start(Path::new(&path), &fixture.bun)?;
        fixture.replacements = transport.replacements;
        fixture.metrics_url = transport.metrics_url;
        Ok((fixture, replay))
    }

    pub(super) fn metrics(&self) -> Result<Metrics, Box<dyn Error>> {
        let output = Command::new("/usr/bin/curl")
            .args(["--max-time", "5", "-fsS", &self.metrics_url])
            .output()?;
        assert!(output.status.success());
        Ok(serde_json::from_slice(&output.stdout)?)
    }

    pub(super) fn deny_archives(&self) -> Result<(), Box<dyn Error>> {
        let url = self.metrics_url.replace("/__metrics", "/__deny-archives");
        let status = Command::new("/usr/bin/curl")
            .args(["--max-time", "5", "-fsS", "-X", "POST", &url])
            .stdout(Stdio::null())
            .status()?;
        assert!(status.success());
        Ok(())
    }

    pub(super) fn allow_archives(&self) -> Result<(), Box<dyn Error>> {
        let url = self.metrics_url.replace("/__metrics", "/__allow-archives");
        let status = Command::new("/usr/bin/curl")
            .args(["--max-time", "5", "-fsS", "-X", "POST", &url])
            .stdout(Stdio::null())
            .status()?;
        assert!(status.success());
        Ok(())
    }

    pub(super) fn install(
        &self,
        sandbox: &Sandbox,
        tool: &str,
        version: &str,
    ) -> Result<(), Box<dyn Error>> {
        let output = self.install_result(sandbox, tool, version)?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }

    pub(super) fn install_result(
        &self,
        sandbox: &Sandbox,
        tool: &str,
        version: &str,
    ) -> Result<std::process::Output, Box<dyn Error>> {
        super::nonrust_process::output(
            Command::new(&self.mise)
                .args([
                    "--no-config",
                    "--no-env",
                    "--no-hooks",
                    "install",
                    &format!("{tool}@{version}"),
                ])
                .env_clear()
                .env("HOME", sandbox.home.path())
                .envs(self.environment(sandbox))
                .current_dir(sandbox.workspace.path()),
        )
    }

    pub(super) fn corrupt_bundles(&self) -> Result<(), Box<dyn Error>> {
        let url = self.metrics_url.replace("/__metrics", "/__corrupt-bundles");
        let status = Command::new("/usr/bin/curl")
            .args(["--max-time", "5", "-fsS", "-X", "POST", &url])
            .stdout(Stdio::null())
            .status()?;
        assert!(status.success());
        Ok(())
    }

    pub(super) fn provision(&self, sandbox: &Sandbox) -> Result<(), Box<dyn Error>> {
        let cache = sandbox
            .home
            .path()
            .join(self.cache.strip_prefix(&self.home)?);
        fs::create_dir_all(&cache)?;
        for name in ["downloads-cas", "github"] {
            copy_tree(&self.cache.join(name), &cache.join(name))?;
        }
        copy_tree(
            &self.home.join(".local/state/mise/packslip"),
            &sandbox.home.path().join(".local/state/mise/packslip"),
        )?;
        for tool in [
            "shellcheck",
            "gh",
            "jactionlint",
            "bun",
            "python",
            "uv",
            "dbt-fusion",
        ] {
            let source = self.cache.join(tool);
            if !source.exists() {
                continue;
            }
            fs::create_dir_all(cache.join(tool))?;
            for entry in fs::read_dir(source)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    fs::copy(entry.path(), cache.join(tool).join(entry.file_name()))?;
                }
            }
        }
        fs::create_dir_all(sandbox.mise_root().join("bin"))?;
        fs::copy(&self.mise, sandbox.mise_root().join("bin/mise"))?;
        assert!(!sandbox.mise_root().join("installs").exists());
        Ok(())
    }

    pub(super) fn environment(&self, sandbox: &Sandbox) -> Vec<(String, String)> {
        let (os, arch) = match self.host.as_str() {
            "linux-x64" => ("Linux", "X64"),
            "macos-arm64" => ("macOS", "ARM64"),
            "macos-x64" => ("macOS", "X64"),
            _ => ("unsupported", "unsupported"),
        };
        vec![
            pair("RUNNER_OS", os),
            pair("RUNNER_ARCH", arch),
            pair("MISE_URL_REPLACEMENTS", &self.replacements),
            pair("MISE_USE_VERSIONS_HOST", "false"),
            pair("MISE_LOCKFILE", "0"),
            pair("MISE_AUTO_INSTALL", "false"),
            pair("MISE_PYTHON_COMPILE", "false"),
            pair("HTTP_PROXY", "http://127.0.0.1:1"),
            pair("HTTPS_PROXY", "http://127.0.0.1:1"),
            pair("ALL_PROXY", "http://127.0.0.1:1"),
            pair("NO_PROXY", "127.0.0.1,localhost"),
            pair(
                "PATH",
                &format!(
                    "{}:/usr/bin:/bin:/usr/sbin:/sbin",
                    sandbox.mise_root().join("bin").display()
                ),
            ),
        ]
    }

    pub(super) fn execute(
        &self,
        sandbox: &Sandbox,
        tool: &str,
        version: &str,
    ) -> Result<(), Box<dyn Error>> {
        let output = super::nonrust_process::output(
            Command::new(&self.mise)
                .args([
                    "--no-config",
                    "--no-env",
                    "--no-hooks",
                    "exec",
                    &format!("{tool}@{version}"),
                    "--",
                    if tool == "dbt-fusion" { "dbt" } else { tool },
                    "--version",
                ])
                .env_clear()
                .env("HOME", sandbox.home.path())
                .envs(self.environment(sandbox))
                .current_dir(sandbox.workspace.path()),
        )?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8(output.stdout)?.contains(version));
        Ok(())
    }
}

pub(super) struct Replay {
    child: Child,
}

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Metrics {
    pub requests: u64,
    pub misses: u64,
    pub archive_requests: u64,
    pub metadata_requests: u64,
    pub bytes: u64,
    pub metadata_bytes: u64,
    pub normalized_bundles: u64,
    pub origin_requests: u64,
    pub origin_github_requests: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransportInfo {
    replacements: String,
    metrics_url: String,
}

impl Replay {
    fn start(path: &Path, bun: &Path) -> Result<(Self, TransportInfo), Box<dyn Error>> {
        let version = super::nonrust_process::output(
            Command::new(bun)
                .args(["--config=/dev/null", "--no-env-file", "--version"])
                .env_remove("BUN_OPTIONS")
                .env_remove("NODE_OPTIONS")
                .env_remove("NODE_PATH"),
        )?;
        assert!(version.status.success());
        assert_eq!(
            String::from_utf8(version.stdout)?.trim(),
            velnor_actions_mise::catalog::BUN_VERSION
        );
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/nonrust-tools-transport.ts");
        let child = Command::new(bun)
            .args(["--config=/dev/null", "--no-env-file"])
            .arg(script)
            .arg(path)
            .env_remove("BUN_OPTIONS")
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .stdout(Stdio::piped())
            .spawn()?;
        let mut replay = Self { child };
        let pipe = replay.child.stdout.take().ok_or("no replay pipe")?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let reader = thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(pipe).read_line(&mut line).map(|_| line);
            drop(sender.send(result));
        });
        let line = match receiver.recv_timeout(Duration::from_secs(10)) {
            Ok(line) => line?,
            Err(error) => {
                replay.child.kill()?;
                replay.child.wait()?;
                reader.join().map_err(|_| "replay reader panicked")?;
                return Err(error.into());
            }
        };
        reader.join().map_err(|_| "replay reader panicked")?;
        if line.trim().is_empty() {
            return Err("replay transport failed to start".into());
        }
        Ok((replay, serde_json::from_str(line.trim())?))
    }
}

impl Drop for Replay {
    fn drop(&mut self) {
        if let Err(error) = self.child.kill() {
            eprintln!("replay cleanup kill: {error}");
        }
        if let Err(error) = self.child.wait() {
            eprintln!("replay cleanup wait: {error}");
        }
    }
}

pub(super) fn copy_tree(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), target)?;
        } else if kind.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(entry.path())?, target)?;
        } else {
            return Err(format!("unexpected fixture entry: {}", entry.path().display()).into());
        }
    }
    Ok(())
}

pub(super) fn tree_bytes(root: &Path) -> Result<u64, Box<dyn Error>> {
    let mut bytes = 0;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_file() {
            bytes += entry.metadata()?.len();
        } else if kind.is_dir() {
            bytes += tree_bytes(&entry.path())?;
        }
    }
    Ok(bytes)
}
