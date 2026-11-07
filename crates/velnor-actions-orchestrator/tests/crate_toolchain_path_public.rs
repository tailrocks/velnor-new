// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(unix)]
#[test]
fn rust_tool_must_be_inside_owned_pinned_toolchain_without_symlinks()
-> Result<(), Box<dyn std::error::Error>> {
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    use velnor_actions_orchestrator::verified_rust_tool_path;
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let home = root.join("rustup");
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Ok(()),
    };
    let bin = home.join(format!("toolchains/1.97.1-{target}/bin"));
    fs::create_dir_all(&bin)?;
    fs::write(bin.join("cargo"), "fixture")?;
    fs::set_permissions(bin.join("cargo"), fs::Permissions::from_mode(0o755))?;
    assert_eq!(
        verified_rust_tool_path(&home, "1.97.1", "cargo")?,
        bin.join("cargo")
    );
    assert!(verified_rust_tool_path(&home, "1.97.2", "cargo").is_err());
    assert!(verified_rust_tool_path(&home, "1.97.1", "rustc").is_err());
    assert!(verified_rust_tool_path(&home, "1.97.1", "../cargo").is_err());
    let linked_home = root.join("linked-rustup");
    symlink(&home, &linked_home)?;
    assert!(verified_rust_tool_path(&linked_home, "1.97.1", "cargo").is_err());
    fs::rename(&bin, home.join("toolchains/elsewhere"))?;
    symlink(home.join("toolchains/elsewhere"), &bin)?;
    assert!(verified_rust_tool_path(&home, "1.97.1", "cargo").is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn owned_homes_must_be_exact_separate_runner_directories() -> Result<(), Box<dyn std::error::Error>>
{
    use std::{fs, os::unix::fs::symlink};
    use velnor_actions_orchestrator::verify_owned_tool_homes;
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let rustup = root.join("velnor/rustup");
    let cargo = root.join("velnor/cargo");
    let source = root.join("source");
    fs::create_dir_all(&rustup)?;
    fs::create_dir_all(&cargo)?;
    fs::create_dir_all(&source)?;
    assert!(verify_owned_tool_homes(&root, &rustup, &rustup, &cargo, &cargo, &source).is_ok());
    assert!(verify_owned_tool_homes(&root, &source, &rustup, &cargo, &cargo, &source).is_err());
    assert!(verify_owned_tool_homes(&root, &rustup, &source, &cargo, &cargo, &source).is_err());
    assert!(verify_owned_tool_homes(&root, &rustup, &rustup, &source, &cargo, &source).is_err());
    assert!(verify_owned_tool_homes(&root, &rustup, &rustup, &cargo, &source, &source).is_err());
    assert!(verify_owned_tool_homes(&root, &rustup, &rustup, &cargo, &cargo, &rustup).is_err());
    assert!(verify_owned_tool_homes(&root, &rustup, &rustup, &cargo, &cargo, &root).is_err());
    let alias = root.join("alias");
    symlink(&root, &alias)?;
    assert!(verify_owned_tool_homes(&alias, &rustup, &rustup, &cargo, &cargo, &source).is_err());
    assert!(
        verify_owned_tool_homes(&root.join(".."), &rustup, &rustup, &cargo, &cargo, &source)
            .is_err()
    );
    Ok(())
}
