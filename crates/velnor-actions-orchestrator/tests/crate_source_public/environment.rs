// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[cfg(unix)]
#[test]
fn metadata_subprocess_drops_ambient_cargo_overrides() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let home =
        std::path::PathBuf::from(std::env::var_os("RUSTUP_HOME").ok_or("missing rustup home")?);
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Ok(()),
    };
    let catalog = velnor_actions_mise::ToolCatalog::pinned();
    let rust = catalog.version(velnor_actions_mise::PinnedTool::Rust);
    let real_bin = home.join(format!("toolchains/{rust}-{target}/bin"));
    let fake_home = root.join("rustup");
    let fake_bin = fake_home.join(format!("toolchains/{rust}-{target}/bin"));
    let fake_mise = root.join("mise-bin");
    fs::create_dir_all(&fake_bin)?;
    fs::create_dir_all(&fake_mise)?;
    let marker = root.join("cargo-reached");
    let cargo_script = format!(
        "#!/bin/sh\nprintf reached > '{}'\n[ -z \\\"${{RUSTC_WRAPPER:-}}\\\" ] || exit 72\n[ -z \\\"${{CARGO_BUILD_RUSTC:-}}\\\" ] || exit 73\nexec '{}' \\\"$@\\\"\n",
        marker.display(),
        real_bin.join("cargo").display()
    );
    let mise = fake_mise.join("mise");
    fs::write(
        &mise,
        "#!/bin/sh\nwhile [ \\\"$1\\\" != -- ]; do shift; done\nshift\nexec \\\"$@\\\"\n"
            .replace("\\\"", "\""),
    )?;
    fs::write(fake_bin.join("cargo"), cargo_script.replace("\\\"", "\""))?;
    for tool in ["rustc", "rustdoc"] {
        fs::write(
            fake_bin.join(tool),
            format!(
                "#!/bin/sh\nexec '{}' \\\"$@\\\"\n",
                real_bin.join(tool).display()
            )
            .replace("\\\"", "\""),
        )?;
    }
    for path in [
        mise,
        fake_bin.join("cargo"),
        fake_bin.join("rustc"),
        fake_bin.join("rustdoc"),
    ] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    let status = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "crate_source_public::approved_source_must_be_exact_clean_repository",
        ])
        .env("PATH", format!("{}:/usr/bin:/bin", fake_mise.display()))
        .env("RUSTUP_HOME", &fake_home)
        .env("RUSTC_WRAPPER", "hostile-sentinel")
        .env("CARGO_BUILD_RUSTC", "hostile-sentinel")
        .status()?;
    assert!(status.success());
    assert_eq!(fs::read_to_string(marker)?, "reached");
    Ok(())
}
