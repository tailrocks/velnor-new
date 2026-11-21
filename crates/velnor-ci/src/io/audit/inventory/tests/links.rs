// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::symlink;

use nix::sys::resource::{Resource, getrlimit, setrlimit};

use super::*;

fn base(name: &str) -> PathBuf {
    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-link-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    base
}

fn link_resolutions(directory: &Path) -> BTreeMap<PathBuf, Option<PathBuf>> {
    let entries = scan_with(
        directory,
        &BTreeSet::new(),
        &mut Budget::new(usize::MAX),
        &Probe::default(),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let children = entries
        .get(&fs::canonicalize(directory).unwrap_or_else(|e| unreachable!("{e}")))
        .unwrap_or_else(|| unreachable!("{} is not recorded", directory.display()));
    children
        .iter()
        .filter_map(|(name, kind)| match kind {
            Kind::Link { resolved, .. } => Some((name.clone(), resolved.clone())),
            Kind::File | Kind::Directory | Kind::Other => None,
        })
        .collect()
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
    let resolutions = link_resolutions(&dir);
    for (name, _) in &links {
        let expected = fs::canonicalize(dir.join(name)).ok();
        assert_eq!(resolutions.get(Path::new(name)), Some(&expected), "{name}");
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
    symlink("../a/file/.", dir.join("dotted")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink("../a/file", dir.join("plain")).unwrap_or_else(|e| unreachable!("{e}"));
    let resolutions = link_resolutions(&dir);
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(resolutions.get(Path::new("slashed")), Some(&None));
    assert_eq!(resolutions.get(Path::new("dotted")), Some(&None));
    assert!(matches!(resolutions.get(Path::new("plain")), Some(Some(_))));
}

#[test]
fn a_link_crossing_more_than_forty_links_is_unresolved_and_one_within_the_limit_resolves() {
    let base = base("hops");
    let dir = base.join("dir");
    fs::create_dir_all(base.join("a/b")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    chain(&dir, "short", 40);
    chain(&dir, "long", 41);
    let resolutions = link_resolutions(&dir);
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(
        resolutions.get(Path::new("short-0")),
        Some(Some(_))
    ));
    assert_eq!(resolutions.get(Path::new("long-0")), Some(&None));
}

#[test]
fn a_link_into_a_deeply_nested_directory_resolves_like_canonicalize() {
    let base = base("deep");
    let dir = base.join("dir");
    let nested: PathBuf = std::iter::repeat_n("d", 400).collect();
    fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(base.join("nested").join(&nested)).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(Path::new("../nested").join(&nested), dir.join("entry"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (soft, hard) = getrlimit(Resource::RLIMIT_NOFILE).unwrap_or_else(|e| unreachable!("{e}"));
    setrlimit(Resource::RLIMIT_NOFILE, hard.min(256), hard).unwrap_or_else(|e| unreachable!("{e}"));
    let resolutions = link_resolutions(&dir);
    setrlimit(Resource::RLIMIT_NOFILE, soft, hard).unwrap_or_else(|e| unreachable!("{e}"));
    let expected = fs::canonicalize(dir.join("entry")).ok();
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(expected.is_some());
    assert_eq!(resolutions.get(Path::new("entry")), Some(&expected));
}
