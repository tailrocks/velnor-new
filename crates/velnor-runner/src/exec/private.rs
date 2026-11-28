// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use tokio::fs::{DirBuilder, OpenOptions};
use tokio::io::AsyncWriteExt;

use super::ExecError;

pub(super) async fn create_dir(path: &Path) -> Result<(), ExecError> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
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

pub(crate) async fn append(path: &Path, contents: &[u8]) -> Result<(), ExecError> {
    let mut options = OpenOptions::new();
    options.append(true);
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
