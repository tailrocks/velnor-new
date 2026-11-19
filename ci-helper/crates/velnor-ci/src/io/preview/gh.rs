// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs::{self, DirBuilder};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use serde_json::Value;

use crate::error::CiError;
use crate::preview::REPOSITORY;
use crate::preview::attest::{Attestor, Authority, Subject, check_verified};
use crate::preview::json::{Res, digest, hex, parse, require};

const MAX_GH_BYTES: u64 = 104_857_600;
const MAX_OUTPUT: usize = 16_777_216;
const RUN_LIMIT: Duration = Duration::from_secs(90);
const LINUX_X86_64: [&str; 2] = [
    "cbfdc94f3a5c0713c0d6df3e0a1d88a59a76b0c52332aad7d21d112f9da2cc0e",
    "b768184570b12536be4ac1528893f6340d1c6a7b40b56ad45bca9cd0e87ad34d",
];
const DARWIN_ARM64: [&str; 2] = [
    "5e4470bb57bf1e7cb5063be940d5dffc5be3e1899d6bf7fe1ff15372f7f4cc29",
    "5f8d7527e2bdf584249b030cdd1550c91d75e8afb40aa3c11573d416817a1260",
];

/// Runs `gh attestation verify` with a pinned `gh` binary.
#[derive(Debug)]
pub struct GhAttestor {
    gh: PathBuf,
    temp_root: PathBuf,
    pinned: BTreeSet<String>,
}

fn error(what: &str) -> CiError {
    CiError::new(format!("source preview attestation: {what}"))
}

impl GhAttestor {
    /// `gh` must be one of the pinned builds for this host.
    pub fn pinned(gh: PathBuf, temp_root: PathBuf) -> Res<Self> {
        let hashes: &[&str] = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") => &LINUX_X86_64,
            ("macos", "aarch64") => &DARWIN_ARM64,
            _ => return Err(error("unsupported host")),
        };
        Ok(Self::with_hashes(
            gh,
            temp_root,
            hashes.iter().map(|h| (*h).to_owned()).collect(),
        ))
    }

    #[must_use]
    pub fn with_hashes(gh: PathBuf, temp_root: PathBuf, pinned: BTreeSet<String>) -> Self {
        Self {
            gh,
            temp_root,
            pinned,
        }
    }

    /// Port of `verify_gh_binary`: an absolute path with no symbolic link, a regular
    /// file of a pinned hash, that this user may run.
    fn check_binary(&self) -> Res<()> {
        require(self.gh.is_absolute())?;
        for parent in self.gh.ancestors() {
            let meta = fs::symlink_metadata(parent).map_err(|_| error("gh path"))?;
            require(!meta.file_type().is_symlink())?;
        }
        let mut file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&self.gh)
            .map_err(|_| error("gh open"))?;
        let meta = file.metadata().map_err(|_| error("gh stat"))?;
        require(meta.is_file() && meta.len() <= MAX_GH_BYTES)?;
        let mut data = Vec::new();
        (&mut file)
            .take(MAX_GH_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|_| error("gh read"))?;
        require(data.len() as u64 <= MAX_GH_BYTES && self.pinned.contains(&digest(&data)))?;
        require(meta.permissions().mode() & 0o111 != 0)
    }
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn scratch(root: &Path) -> Res<Scratch> {
    let mut random = [0_u8; 8];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut random))
        .map_err(|_| error("random"))?;
    let name = hex(&random);
    let path = root.join(format!("source-preview-stage-{name}"));
    DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .map_err(|_| error("scratch"))?;
    Ok(Scratch(path))
}

/// Run to completion, capture at most 16 MiB of standard output, and kill the whole
/// process group at the deadline or on any failure.
fn capture(mut command: Command, deadline: Instant) -> Res<Vec<u8>> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = command.spawn().map_err(|_| error("spawn"))?;
    let group = Pid::from_raw(i32::try_from(child.id()).map_err(|_| error("pid"))?);
    let outcome = (|| -> Res<Vec<u8>> {
        let mut stdout = child.stdout.take().ok_or_else(|| error("pipe"))?;
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut out = Vec::new();
            let mut chunk = vec![0_u8; 65_536];
            let result = loop {
                match stdout.read(&mut chunk) {
                    Ok(0) => break Ok(out),
                    Ok(n) => {
                        out.extend_from_slice(&chunk[..n]);
                        if out.len() > MAX_OUTPUT {
                            break Err(());
                        }
                    }
                    Err(_) => break Err(()),
                }
            };
            drop(sender.send(result));
        });
        let wait = deadline.saturating_duration_since(Instant::now());
        let output = receiver
            .recv_timeout(wait)
            .map_err(|_| error("timeout"))?
            .map_err(|()| error("output"))?;
        loop {
            if let Some(status) = child.try_wait().map_err(|_| error("wait"))? {
                require(status.success())?;
                return Ok(output);
            }
            require(Instant::now() < deadline)?;
            thread::sleep(Duration::from_millis(5));
        }
    })();
    if child.try_wait().ok().flatten().is_none() {
        killpg(group, Signal::SIGKILL).unwrap_or_default();
        drop(child.wait());
    }
    outcome
}

impl Attestor for GhAttestor {
    fn verify(
        &self,
        receipt: &[u8],
        bundle: &[u8],
        subject: &Subject,
        authority: &Authority,
        deadline: Instant,
    ) -> Res<Value> {
        require(authority.sha != subject.source_sha)?;
        self.check_binary()?;
        let dir = scratch(&self.temp_root)?;
        let receipt_path = dir.0.join("SDK_PREVIEW_RECEIPT.json");
        let bundle_path = dir.0.join("SDK_PREVIEW_RECEIPT.json.sigstore.jsonl");
        fs::write(&receipt_path, receipt).map_err(|_| error("write"))?;
        fs::write(&bundle_path, bundle).map_err(|_| error("write"))?;
        let mut command = Command::new(&self.gh);
        command
            .args(["attestation", "verify"])
            .arg(&receipt_path)
            .arg("--bundle")
            .arg(&bundle_path)
            .args(["--repo", REPOSITORY, "--signer-workflow"])
            .arg(format!(
                "{REPOSITORY}/{}",
                crate::preview::AUTHORITY_WORKFLOW
            ))
            .args([
                "--signer-digest",
                &authority.sha,
                "--source-digest",
                &subject.source_sha,
            ])
            .args(["--source-ref", &subject.source_ref(), "--format", "json"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("GH_PROMPT_DISABLED", "1");
        for key in ["HOME", "LANG", "LC_ALL", "TMPDIR"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let limit = deadline.min(Instant::now() + RUN_LIMIT);
        let output = capture(command, limit)?;
        let verified = parse(&output, MAX_OUTPUT)?;
        check_verified(&verified, subject, authority)?;
        Ok(verified)
    }
}
