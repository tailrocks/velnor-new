// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::io::AsyncReadExt;

use crate::cli::{EnvironmentArgs, RunnerMode};

const FLAG: &str = "--velnor-ci-binary";
const MAX_BYTES: u64 = 256 << 20;
const ELF_HEADER: usize = 20;
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELF_CLASS_64: u8 = 2;
const ELF_LITTLE_ENDIAN: u8 = 1;
const ELF_MACHINE_AARCH64: [u8; 2] = [0xb7, 0];
const TARBALLS: &str =
    "Runner.Server__ActionDownloadUrls__0__TarballUrl=http://api-stub/repos/{0}/tarball/{1}";
const MOUNT_DIR: &str = "/opt/velnor/local";

fn refused(reason: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("{FLAG} {reason}"))
}

async fn checked(path: &Path) -> io::Result<PathBuf> {
    let path = fs::canonicalize(path)
        .await
        .map_err(|error| refused(format!("{}: {error}", path.display())))?;
    let shown = path.display().to_string();
    if shown.contains([':', ',', '\n']) {
        return Err(refused(format!(
            "{shown} has a ':', ',' or newline, which docker -v cannot mount"
        )));
    }
    let meta = fs::metadata(&path).await?;
    if !meta.is_file() || meta.mode() & 0o111 == 0 {
        return Err(refused(format!(
            "{shown} is not an executable regular file"
        )));
    }
    if meta.len() > MAX_BYTES {
        return Err(refused(format!("{shown} is larger than {MAX_BYTES} bytes")));
    }
    let mut header = [0_u8; ELF_HEADER];
    let mut file = fs::File::open(&path).await?;
    let read = file.read_exact(&mut header).await;
    let elf = read.is_ok()
        && header[..4] == ELF_MAGIC
        && header[4] == ELF_CLASS_64
        && header[5] == ELF_LITTLE_ENDIAN
        && header[18..20] == ELF_MACHINE_AARCH64;
    if !elf {
        return Err(refused(format!(
            "{shown} is not a linux-arm64 ELF executable, which is what the runner containers run"
        )));
    }
    Ok(path)
}

/// What `--velnor-ci-binary` adds to the containers, each as docker arguments. Empty without it.
#[derive(Debug, Default)]
pub(super) struct LocalCi {
    /// For the trusted runners: the binary, read-only.
    pub(super) runner: Vec<String>,
    /// For the trusted runner.server: where it sends action downloads.
    pub(super) standin: Vec<String>,
    /// For the api-stub: serve the local variant of setup-velnor.
    pub(super) stub: Vec<String>,
}

pub(super) async fn flags(args: &EnvironmentArgs) -> io::Result<LocalCi> {
    let Some(binary) = &args.velnor_ci_binary else {
        return Ok(LocalCi::default());
    };
    if args.runner != RunnerMode::Plain {
        return Err(refused(
            "needs --runner plain: velnor-host owns the pair containers and mounts nothing from the host into them",
        ));
    }
    let binary = checked(binary).await?;
    Ok(LocalCi {
        runner: vec![
            "-v".to_owned(),
            format!("{}:{MOUNT_DIR}/velnor-ci:ro", binary.display()),
        ],
        standin: vec!["-e".to_owned(), TARBALLS.to_owned()],
        stub: vec!["--local-setup-velnor".to_owned()],
    })
}
