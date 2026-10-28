// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, os::unix::fs::PermissionsExt, path::Path};

use velnor_actions_mise::{PinnedTool, ToolCatalog};

pub(super) fn write(root: &Path, repo: &Path) -> Result<(), Box<dyn Error>> {
    let catalog = ToolCatalog::pinned();
    let script = include_str!("mise.sh")
        .replace("@RUST_VERSION@", catalog.version(PinnedTool::Rust))
        .replace("@MBX_VERSION@", catalog.version(PinnedTool::MrBoxington))
        .replace(
            "@SHELLCHECK_VERSION@",
            catalog.version(PinnedTool::Shellcheck),
        )
        .replace("@RUST_SPEC@", &catalog.tool_spec(PinnedTool::Rust))
        .replace("@MBX_SPEC@", &catalog.tool_spec(PinnedTool::MrBoxington))
        .replace(
            "@JACTIONLINT_SPEC@",
            &catalog.tool_spec(PinnedTool::Jactionlint),
        )
        .replace(
            "@SHELLCHECK_SPEC@",
            &catalog.tool_spec(PinnedTool::Shellcheck),
        )
        .replace(
            "@MANIFEST_PATH@",
            &repo.join("Cargo.toml").display().to_string(),
        );
    let path = root.join("bin/mise");
    fs::write(&path, script)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
