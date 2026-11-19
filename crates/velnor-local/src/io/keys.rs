// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::Permissions;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use tokio::fs;

use super::pair::{random_hex, write_private};

const TOKEN_HEX_LEN: usize = 64;

pub(super) async fn control(path: &Path) -> io::Result<()> {
    let kept = fs::read_to_string(path).await.unwrap_or_default();
    if !(kept.len() == TOKEN_HEX_LEN && kept.bytes().all(|byte| byte.is_ascii_hexdigit())) {
        write_private(path, &random_hex(TOKEN_HEX_LEN / 2)?).await?;
    }
    fs::set_permissions(path, Permissions::from_mode(0o600)).await
}

pub(super) async fn fresh(path: &Path) -> io::Result<String> {
    let token = random_hex(TOKEN_HEX_LEN / 2)?;
    write_private(path, &token).await?;
    Ok(token)
}
