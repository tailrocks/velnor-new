// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::symlink;

use super::handle::{self, Climb, Origin};
use super::resolve::Reach;
use super::*;

fn base(name: &str) -> PathBuf {
    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-link-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    base
}

fn link_kind(directory: &Path, name: &str) -> Kind {
    let opened = handle::open(&Origin::Path, directory, &Climb::none())
        .unwrap_or_else(|e| unreachable!("{e}"));
    handle::kind_at(
        &opened,
        name.as_ref(),
        &directory.join(name),
        Reach::default(),
    )
    .unwrap_or_else(|e| unreachable!("{e}"))
    .unwrap_or_else(|| unreachable!("{name} is missing"))
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
    let opened =
        handle::open(&Origin::Path, &held, &Climb::none()).unwrap_or_else(|e| unreachable!("{e}"));
    fs::rename(&held, base.join("d.old")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::rename(&other, &held).unwrap_or_else(|e| unreachable!("{e}"));
    let kind = handle::kind_at(&opened, "l".as_ref(), &held.join("l"), Reach::default());
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        kind.unwrap_or_else(|e| unreachable!("{e}")),
        Some(Kind::Link {
            target: PathBuf::from("real"),
            resolved: Some(held.join("real")),
        })
    );
}

#[test]
fn handle_relative_resolution_agrees_with_canonicalize_on_a_stable_tree() {
    let base = base("stable");
    let dir = base.join("dir");
    for sub in ["a/b", "x", "dir/inner"] {
        fs::create_dir_all(base.join(sub)).unwrap_or_else(|e| unreachable!("{e}"));
    }
    fs::write(base.join("a/file"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    let absolute = base.join("a/b");
    let links: [(&str, PathBuf); 10] = [
        ("plain", "inner".into()),
        ("up", "../a/b".into()),
        ("dots", "./inner/../inner/.".into()),
        ("absolute", absolute),
        ("hop", "plain".into()),
        ("slashed", "inner/".into()),
        ("file", "../a/file".into()),
        ("dangling", "nowhere".into()),
        ("through-file", "../a/file/more".into()),
        ("loop", "loop".into()),
    ];
    for (name, target) in &links {
        symlink(target, dir.join(name)).unwrap_or_else(|e| unreachable!("{e}"));
    }
    for (name, _) in &links {
        let expected = fs::canonicalize(dir.join(name)).ok();
        let Kind::Link { resolved, .. } = link_kind(&dir, name) else {
            unreachable!("{name} is not a link")
        };
        assert_eq!(resolved, expected, "{name}");
    }
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
}

fn chain(directory: &Path, prefix: &str, links: usize) {
    for index in 0..links {
        let target = if index + 1 == links {
            "../a/b".to_owned()
        } else {
            format!("{prefix}-{}", index + 1)
        };
        symlink(target, directory.join(format!("{prefix}-{index}")))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
}

#[test]
fn a_link_to_a_file_written_with_a_trailing_slash_is_unresolved() {
    let base = base("slashed-file");
    let dir = base.join("dir");
    fs::create_dir_all(base.join("a")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(base.join("a/file"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    symlink("../a/file/", dir.join("slashed")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink("../a/file", dir.join("plain")).unwrap_or_else(|e| unreachable!("{e}"));
    let slashed = link_kind(&dir, "slashed");
    let plain = link_kind(&dir, "plain");
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(slashed, Kind::Link { resolved: None, .. }),
        "{slashed:?}"
    );
    assert!(
        matches!(
            plain,
            Kind::Link {
                resolved: Some(_),
                ..
            }
        ),
        "{plain:?}"
    );
}

#[test]
fn a_link_crossing_more_than_forty_links_is_unresolved_and_forty_resolves() {
    let base = base("hops");
    let dir = base.join("dir");
    fs::create_dir_all(base.join("a/b")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    chain(&dir, "short", 40);
    chain(&dir, "long", 41);
    let short = link_kind(&dir, "short-0");
    let long = link_kind(&dir, "long-0");
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(
            short,
            Kind::Link {
                resolved: Some(_),
                ..
            }
        ),
        "{short:?}"
    );
    assert!(
        matches!(long, Kind::Link { resolved: None, .. }),
        "{long:?}"
    );
}
