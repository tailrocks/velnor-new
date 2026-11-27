// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use ring::digest;
use rustix::fs::{FileType, OFlags, fstat};
use rustix::io::Errno;
use tokio::io::AsyncReadExt;

use super::inputs::MiseInputs;
use super::tools::open_tools;
use crate::exec::ExecError;
use crate::expr::hex_digest;

const BINARY: &str = "mise";

fn refused(binary: &Path, why: &str) -> ExecError {
    ExecError::Invalid(format!(
        "mise policy: {} {why}; the tools store holds only real files",
        binary.display()
    ))
}

async fn sha256_hex(file: std::fs::File, binary: &Path) -> Result<String, ExecError> {
    let io = |source| ExecError::io(format!("read {}", binary.display()), source);
    let mut file = tokio::fs::File::from_std(file);
    let mut context = digest::Context::new(&digest::SHA256);
    let mut chunk = vec![0_u8; 1 << 16];
    loop {
        let read = file.read(&mut chunk).await.map_err(io)?;
        if read == 0 {
            break;
        }
        context.update(&chunk[..read]);
    }
    Ok(hex_digest(context.finish().as_ref()))
}

pub(super) async fn locate_binary(inputs: &MiseInputs, tools: &Path) -> Result<PathBuf, ExecError> {
    let binary = tools.join(BINARY).join(&inputs.version).join(BINARY);
    let missing = || {
        ExecError::Invalid(format!(
            "mise {} is not in the tools store at {}; run the tools store task first",
            inputs.version,
            binary.display()
        ))
    };
    let held = open_tools(tools)?.ok_or_else(missing)?;
    let relative = Path::new(BINARY).join(&inputs.version).join(BINARY);
    let descriptor = held
        .open_below(&relative, OFlags::RDONLY)
        .map_err(|error| match error {
            Errno::NOENT => missing(),
            Errno::LOOP | Errno::NOTDIR => refused(&binary, "is reached through a link"),
            other => ExecError::io(format!("open {}", binary.display()), other.into()),
        })?;
    let stats = fstat(&descriptor)
        .map_err(|error| ExecError::io(format!("stat {}", binary.display()), error.into()))?;
    if FileType::from_raw_mode(stats.st_mode) != FileType::RegularFile {
        return Err(refused(&binary, "is not a regular file"));
    }
    if !inputs.sha256.is_empty() {
        let actual = sha256_hex(descriptor.into(), &binary).await?;
        if !actual.eq_ignore_ascii_case(&inputs.sha256) {
            return Err(ExecError::Invalid(format!(
                "mise {} sha256 mismatch",
                inputs.version
            )));
        }
    }
    Ok(binary)
}
