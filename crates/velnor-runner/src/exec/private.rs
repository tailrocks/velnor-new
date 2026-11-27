// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use tokio::fs::{DirBuilder, OpenOptions};
use tokio::io::AsyncWriteExt;

use super::ExecError;

pub(crate) const PRIVATE_DIR_MODE: u32 = 0o700;

pub(crate) async fn create_dir_with_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(mode)
        .create(path)
        .await
}

pub(super) async fn create_dir(path: &Path) -> Result<(), ExecError> {
    create_dir_with_mode(path, PRIVATE_DIR_MODE)
        .await
        .map_err(|source| ExecError::io(format!("create {}", path.display()), source))
}

pub(super) async fn write_new(path: &Path, contents: &[u8]) -> Result<(), ExecError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    write(options, path, contents).await
}

pub(super) async fn write_replacing(path: &Path, contents: &[u8]) -> Result<(), ExecError> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true).mode(0o600);
    write(options, path, contents).await
}

async fn write(options: OpenOptions, path: &Path, contents: &[u8]) -> Result<(), ExecError> {
    let context = || format!("write {}", path.display());
    let mut file = options
        .open(path)
        .await
        .map_err(|source| ExecError::io(context(), source))?;
    file.write_all(contents)
        .await
        .map_err(|source| ExecError::io(context(), source))?;
    file.flush()
        .await
        .map_err(|source| ExecError::io(context(), source))
}
