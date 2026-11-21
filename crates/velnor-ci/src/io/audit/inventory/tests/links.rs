// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::symlink;

use super::handle::{self, Origin};
use super::*;

fn base(name: &str) -> PathBuf {
    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-link-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    base
}

#[test]
fn a_link_target_is_resolved_through_the_opened_directory_not_the_path() {
    let base = base("swapped");
    let held = base.join("d");
    fs::create_dir_all(held.join("real")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink("real", held.join("l")).unwrap_or_else(|e| unreachable!("{e}"));
    let other = base.join("d2");
    fs::create_dir_all(other.join("other")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink("other", other.join("l")).unwrap_or_else(|e| unreachable!("{e}"));
    let opened = handle::open(&Origin::Path, &held).unwrap_or_else(|e| unreachable!("{e}"));
    fs::rename(&held, base.join("d.old")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::rename(&other, &held).unwrap_or_else(|e| unreachable!("{e}"));
    let kind = handle::kind_at(&opened, "l".as_ref(), &held.join("l"));
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        kind.unwrap_or_else(|e| unreachable!("{e}")),
        Some(Kind::Link {
            target: PathBuf::from("real"),
            resolved: Some(held.join("real")),
        })
    );
}
