// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::process::Command;

#[test]
fn qualification_tree_identity_preserves_modes_and_escaped_link_bytes() -> Result<(), Box<dyn Error>>
{
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    let file = root.join("a >\\\n$;");
    fs::write(&file, b"abc")?;
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644))?;
    let executable = root.join("z");
    fs::write(&executable, b"abc")?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o601))?;
    let link = root.join("link");
    symlink(
        OsString::from_vec(vec![b'x', b'>', b'\\', b'\n', 0x80]),
        &link,
    )?;
    let run = |mode: &str| {
        Command::new(env!("CARGO_BIN_EXE_velnor-archive-guard"))
            .arg(mode)
            .arg(root)
            .output()
    };
    let baseline = run("tree-identity")?;
    assert!(
        baseline.status.success(),
        "{}",
        String::from_utf8_lossy(&baseline.stderr)
    );
    let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(baseline.stdout, format!("- {digest}  ./a \\x3e\\x5c\\x0a$;\nx {digest}  ./z\nlink ./link -> x\\x3e\\x5c\\x0a\\x80\n").as_bytes());
    let links = run("link-identity")?;
    assert!(links.status.success());
    assert_eq!(links.stdout, b"./link -> x\\x3e\\x5c\\x0a\\x80\n");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o744))?;
    let changed = run("tree-identity")?;
    assert!(changed.status.success());
    assert_ne!(changed.stdout, baseline.stdout);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644))?;
    fs::write(&file, b"changed")?;
    let changed = run("tree-identity")?;
    assert!(changed.status.success());
    assert_ne!(changed.stdout, baseline.stdout);
    fs::write(&file, b"abc")?;
    fs::remove_file(&link)?;
    symlink("different", &link)?;
    let changed = run("tree-identity")?;
    assert!(changed.status.success());
    assert_ne!(changed.stdout, baseline.stdout);
    #[cfg(target_os = "linux")]
    {
        let invalid = root.join(OsString::from_vec(vec![0xff]));
        fs::write(invalid, b"x")?;
        let rejected = run("tree-identity")?;
        assert!(!rejected.status.success());
        assert!(String::from_utf8_lossy(&rejected.stderr).contains("tree path is not UTF-8"));
    }
    Ok(())
}
