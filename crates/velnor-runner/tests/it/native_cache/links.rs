// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use anyhow::Result;

use crate::exec_support::Scratch;
use crate::native_cache_support::{Store, overwrite_entry, run, run_in, saved, tar_member};

#[test]
fn restore_keeps_symlink_pointing_outside_declared_path() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    overwrite_entry(
        &store,
        &[
            tar_member(b"0/shims/", b'5', b"", b""),
            tar_member(b"0/shims/cargo", b'2', b"/opt/mise/bin/mise", b""),
            tar_member(b"0/shims/up", b'2', b"../../outside", b""),
        ],
    )?;
    let restored = run("cache-restore-exact", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    let shims = restored.workspace_file("data/shims");
    assert_eq!(
        fs::read_link(shims.join("cargo"))?,
        Path::new("/opt/mise/bin/mise")
    );
    assert_eq!(fs::read_link(shims.join("up"))?, Path::new("../../outside"));
    Ok(())
}

#[test]
fn restore_never_writes_through_symlink() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let scenarios = [
        vec![
            tar_member(b"0/a", b'2', b"../outside", b""),
            tar_member(b"0/a/b", b'0', b"", b"x"),
        ],
        vec![tar_member(b"0/sub/escape.txt", b'0', b"", b"x")],
        vec![
            tar_member(b"0/sub/", b'5', b"", b""),
            tar_member(b"0/sub/escape.txt", b'0', b"", b"x"),
        ],
    ];
    for members in &scenarios {
        overwrite_entry(&store, members)?;
        let work = Scratch::new()?;
        let target = work.workspace().join("outside");
        fs::create_dir_all(&target)?;
        fs::write(target.join("victim.txt"), "untouched")?;
        fs::create_dir_all(work.workspace().join("data"))?;
        symlink(&target, work.workspace().join("data/sub"))?;
        let restored = run_in(work, "cache-restore-exact", &store, &[])?;
        assert_eq!(restored.ran.code, Some(1), "{}", restored.ran.stdout);
        assert!(
            restored
                .ran
                .stdout
                .contains("cache restore never writes through a link"),
            "{}",
            restored.ran.stdout
        );
        let names: Vec<_> = fs::read_dir(&target)?
            .map(|item| item.map(|item| item.file_name()))
            .collect::<Result<_, _>>()?;
        assert_eq!(names, ["victim.txt"]);
    }
    overwrite_entry(&store, &[tar_member(b"0/victim", b'0', b"", b"x")])?;
    let work = Scratch::new()?;
    let victim = work.workspace().join("victim.txt");
    fs::create_dir_all(work.workspace().join("data"))?;
    fs::write(&victim, "untouched")?;
    symlink(&victim, work.workspace().join("data/victim"))?;
    let restored = run_in(work, "cache-restore-exact", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    assert_eq!(fs::read_to_string(&victim)?, "untouched");
    let replaced = restored.workspace_file("data/victim");
    assert!(!fs::symlink_metadata(&replaced)?.is_symlink());
    assert_eq!(fs::read_to_string(replaced)?, "x");
    Ok(())
}
