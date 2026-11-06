// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(unix)]
#[test]
fn guard_emits_collector_identity_for_modes_and_escaped_links()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::{
        ffi::OsStringExt,
        fs::{PermissionsExt, symlink},
    };
    let root = tempfile::tempdir()?;
    std::fs::write(root.path().join("a>\n"), b"content")?;
    let executable = root.path().join("executable");
    std::fs::write(&executable, b"program")?;
    std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o755))?;
    symlink(
        std::ffi::OsString::from_vec(b"dir>\n\\\xff".to_vec()),
        root.path().join("link"),
    )?;
    let guard = env!("CARGO_BIN_EXE_velnor-archive-guard");
    let output = std::process::Command::new(guard)
        .arg("qualification-golden-tree")
        .arg(root.path())
        .output()?;
    assert!(output.status.success());
    let expected = concat!(
        "- ed7002b439e9ac845f22357d822bac1444730fbdb6016d3ec9432297b9ec9f73  ./a\\x3e\\x0a\n",
        "x 1310ca2c8932dbd118668bd97442558a3e1f546b5e15c69699dfbd6024f548d8  ./executable\n",
        "link ./link -> dir\\x3e\\x0a\\x5c\\xff\n"
    );
    assert_eq!(output.stdout, expected.as_bytes());
    let links = std::process::Command::new(guard)
        .arg("qualification-golden-links")
        .arg(root.path())
        .output()?;
    assert!(links.status.success());
    assert_eq!(links.stdout, b"./link -> dir\\x3e\\x0a\\x5c\\xff\n");
    Ok(())
}
