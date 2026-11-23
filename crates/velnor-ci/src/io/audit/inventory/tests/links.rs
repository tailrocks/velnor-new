// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::symlink;

use super::*;

fn base(name: &str) -> PathBuf {
    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-link-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    base
}

fn scan_links(directory: &Path) -> Entries {
    scan_with(
        directory,
        &BTreeSet::new(),
        &mut Budget::new(usize::MAX),
        &Probe::default(),
    )
    .unwrap_or_else(|e| unreachable!("{e}"))
}

fn link_at<'a>(entries: &'a Entries, directory: &Path, name: &str) -> &'a Kind {
    entries
        .get(directory)
        .and_then(|children| children.get(Path::new(name)))
        .unwrap_or_else(|| unreachable!("{name} is missing from {directory:?}"))
}

#[test]
fn a_link_resolves_to_its_canonical_path_on_a_stable_tree() {
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
    let entries = scan_links(&dir);
    for (name, _) in &links {
        let expected = fs::canonicalize(dir.join(name)).ok();
        let Kind::Link { resolved, .. } = link_at(&entries, &dir, name) else {
            unreachable!("{name} is not a link")
        };
        assert_eq!(resolved, &expected, "{name}");
    }
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
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
    let entries = scan_links(&dir);
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    let slashed = link_at(&entries, &dir, "slashed");
    let plain = link_at(&entries, &dir, "plain");
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
fn a_link_crossing_more_than_forty_links_is_unresolved_and_forty_resolves() {
    let base = base("hops");
    let dir = base.join("dir");
    fs::create_dir_all(base.join("a/b")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    chain(&dir, "short", 40);
    chain(&dir, "long", 41);
    let entries = scan_links(&dir);
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    let short = link_at(&entries, &dir, "short-0");
    let long = link_at(&entries, &dir, "long-0");
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
