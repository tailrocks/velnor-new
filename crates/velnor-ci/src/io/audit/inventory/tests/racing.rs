// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
fn scan_racing(
    name: &str,
    before_open: &dyn Fn(&Path) -> io::Result<()>,
    after_open: &dyn Fn(&Path),
) -> io::Result<Entries> {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-scan-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(root.join("kept.txt"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    let mut budget = Budget::new(usize::MAX);
    let result = scan_with(
        &root,
        &BTreeSet::new(),
        &mut budget,
        &Probe {
            before_open: &|directory| {
                if directory.ends_with("sub") {
                    before_open(directory)
                } else {
                    Ok(())
                }
            },
            after_open: &|directory| {
                if directory.ends_with("sub") {
                    after_open(directory);
                }
            },
            ..Probe::default()
        },
    );
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    result
}

fn root_children(entries: &Entries) -> &BTreeMap<PathBuf, Kind> {
    entries
        .values()
        .next()
        .unwrap_or_else(|| unreachable!("root is always recorded"))
}

#[test]
fn a_directory_replaced_by_a_regular_file_before_it_is_opened_is_recorded_as_that_file() {
    let entries = scan_racing(
        "replaced-file",
        &|directory| {
            fs::remove_dir(directory)?;
            fs::write(directory, b"")
        },
        &|_| {},
    );
    let entries =
        entries.unwrap_or_else(|e| panic!("scan failed on a directory replaced by a file: {e}"));
    let children = root_children(&entries);
    assert_eq!(
        children.get(Path::new("sub")),
        Some(&Kind::File),
        "{children:?}"
    );
    assert_eq!(entries.len(), 1, "{:?}", entries.keys());
}

#[test]
fn a_directory_replaced_by_a_symbolic_link_before_it_is_opened_is_recorded_as_that_link() {
    let entries = scan_racing(
        "replaced-link",
        &|directory| {
            let target = directory.with_file_name("kept.txt");
            fs::remove_dir(directory)?;
            std::os::unix::fs::symlink(target, directory)
        },
        &|_| {},
    );
    let entries =
        entries.unwrap_or_else(|e| panic!("scan failed on a directory replaced by a link: {e}"));
    let children = root_children(&entries);
    assert!(
        matches!(children.get(Path::new("sub")), Some(Kind::Link { .. })),
        "{children:?}"
    );
    assert_eq!(entries.len(), 1, "{:?}", entries.keys());
}

#[test]
fn a_directory_replaced_by_a_link_to_itself_before_it_is_opened_is_recorded_as_that_link() {
    let entries = scan_racing(
        "replaced-loop",
        &|directory| {
            fs::remove_dir(directory)?;
            std::os::unix::fs::symlink(directory, directory)
        },
        &|_| {},
    );
    let entries = entries
        .unwrap_or_else(|e| panic!("scan failed on a directory replaced by a cyclic link: {e}"));
    let children = root_children(&entries);
    assert!(
        matches!(
            children.get(Path::new("sub")),
            Some(Kind::Link { resolved: None, .. })
        ),
        "{children:?}"
    );
    assert_eq!(entries.len(), 1, "{:?}", entries.keys());
}

fn assert_link_to_directory_recorded(entries: io::Result<Entries>) {
    let entries = entries.unwrap_or_else(|e| {
        panic!("scan failed on a directory replaced by a link to a directory: {e}")
    });
    let children = root_children(&entries);
    assert!(
        matches!(
            children.get(Path::new("sub")),
            Some(Kind::Link {
                resolved: Some(_),
                ..
            })
        ),
        "the replaced directory must be recorded as the link it now is: {children:?}"
    );
    assert!(
        entries.keys().any(|path| path.ends_with("elsewhere")),
        "the link target must be walked as a stable scan would: {:?}",
        entries.keys()
    );
    assert!(
        !entries.keys().any(|path| path.ends_with("sub")),
        "the link must not be recorded as a directory: {:?}",
        entries.keys()
    );
}

fn replace_with_link_to_directory(directory: &Path) -> io::Result<()> {
    let elsewhere = directory.with_file_name("elsewhere");
    fs::create_dir(&elsewhere)?;
    fs::write(elsewhere.join("inner.txt"), b"")?;
    fs::remove_dir(directory)?;
    std::os::unix::fs::symlink(elsewhere, directory)
}

#[test]
fn a_directory_replaced_by_a_link_to_a_directory_before_it_is_opened_is_recorded_as_that_link() {
    assert_link_to_directory_recorded(scan_racing(
        "replaced-dir-link",
        &replace_with_link_to_directory,
        &|_| {},
    ));
}

#[test]
fn a_directory_replaced_by_a_link_to_a_directory_just_after_it_is_opened_is_recorded_as_that_link()
{
    assert_link_to_directory_recorded(scan_racing(
        "opened-then-replaced",
        &|_| Ok(()),
        &|directory| {
            replace_with_link_to_directory(directory).unwrap_or_else(|e| unreachable!("{e}"));
        },
    ));
}

#[test]
fn a_directory_swapped_for_a_new_directory_after_it_is_opened_fails_the_scan() {
    let entries = scan_racing("swapped-directory", &|_| Ok(()), &|directory| {
        let aside = directory.with_file_name("sub-old");
        fs::rename(directory, aside).unwrap_or_else(|e| unreachable!("{e}"));
        fs::create_dir(directory).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(directory.join("new.txt"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    });
    assert!(
        matches!(&entries, Err(error) if error.to_string().contains("was replaced by another directory while it was listed")),
        "a directory swapped after it was opened must fail the scan, not record the old listing: {entries:?}"
    );
}

#[test]
fn a_directory_that_is_a_directory_again_before_it_is_opened_still_fails_the_scan() {
    let entries = scan_racing(
        "directory-again",
        &|directory| {
            fs::remove_dir(directory)?;
            fs::create_dir(directory)?;
            Err(io::Error::from(io::ErrorKind::NotFound))
        },
        &|_| {},
    );
    assert!(
        matches!(&entries, Err(error) if error.kind() == io::ErrorKind::NotFound),
        "{entries:?}"
    );
}

#[test]
fn a_subdirectory_is_descended_through_its_opened_parent_not_its_path() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-scan-parent-swap-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let parent = root.join("d");
    fs::create_dir_all(parent.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(parent.join("sub").join("original.txt"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    let swapped = AtomicBool::new(false);
    let mut budget = Budget::new(usize::MAX);
    let entries = scan_with(
        &root,
        &BTreeSet::new(),
        &mut budget,
        &Probe {
            before_open: &|directory| {
                if directory.ends_with("d/sub") && !swapped.swap(true, Ordering::Relaxed) {
                    let parent = directory.with_file_name("");
                    fs::rename(&parent, root.join("d.aside"))
                        .unwrap_or_else(|e| unreachable!("{e}"));
                    fs::create_dir_all(parent.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
                    fs::write(parent.join("sub").join("replacement.txt"), b"")
                        .unwrap_or_else(|e| unreachable!("{e}"));
                }
                Ok(())
            },
            ..Probe::default()
        },
    );
    let swapped = swapped.load(Ordering::Relaxed);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(swapped, "the parent was never swapped");
    let entries =
        entries.unwrap_or_else(|e| panic!("scan failed after the parent was swapped: {e}"));
    let sub = entries
        .get(&root.join("d").join("sub"))
        .unwrap_or_else(|| unreachable!("{:?}", entries.keys()));
    assert!(sub.contains_key(Path::new("original.txt")), "{sub:?}");
    assert!(
        !sub.contains_key(Path::new("replacement.txt")),
        "walked the replacement directory: {sub:?}"
    );
}

#[test]
fn a_directory_removed_after_it_is_opened_is_dropped_from_the_inventory() {
    let entries = scan_racing("removed-after-open", &|_| Ok(()), &|directory| {
        fs::remove_dir(directory).unwrap_or_else(|e| unreachable!("{e}"));
    });
    let entries = entries.unwrap_or_else(|e| panic!("scan failed on a vanished directory: {e}"));
    let children = root_children(&entries);
    assert!(!children.contains_key(Path::new("sub")), "{children:?}");
    assert!(children.contains_key(Path::new("kept.txt")), "{children:?}");
    assert_eq!(entries.len(), 1, "{:?}", entries.keys());
}

fn scan_with_root_replaced_after_it_is_opened(
    name: &str,
    replace: fn(&Path, &Path),
) -> io::Result<Entries> {
    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!(
            "velnor-scan-root-swap-{name}-{}",
            std::process::id()
        ));
    let _ = fs::remove_dir_all(&base);
    let root = base.join("root");
    fs::create_dir_all(root.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
    let aside = base.join("aside");
    let mut budget = Budget::new(usize::MAX);
    let result = scan_with(
        &root,
        &BTreeSet::new(),
        &mut budget,
        &Probe {
            after_open: &|directory| {
                if directory.ends_with("root") {
                    replace(directory, &aside);
                }
            },
            ..Probe::default()
        },
    );
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    result
}

#[test]
fn a_root_removed_while_it_is_listed_fails_the_scan() {
    let entries = scan_with_root_replaced_after_it_is_opened("removed", |root, aside| {
        fs::rename(root, aside).unwrap_or_else(|e| unreachable!("{e}"));
    });
    assert!(
        matches!(&entries, Err(error) if error.to_string().contains("was replaced while it was listed")),
        "{entries:?}"
    );
}

#[test]
fn a_root_replaced_by_a_file_while_it_is_listed_fails_the_scan() {
    let entries = scan_with_root_replaced_after_it_is_opened("replaced-by-file", |root, aside| {
        fs::rename(root, aside).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(root, b"").unwrap_or_else(|e| unreachable!("{e}"));
    });
    assert!(
        matches!(&entries, Err(error) if error.to_string().contains("was replaced while it was listed")),
        "{entries:?}"
    );
}

#[test]
fn a_root_replaced_by_a_directory_while_it_is_listed_fails_the_scan() {
    let entries =
        scan_with_root_replaced_after_it_is_opened("replaced-by-directory", |root, aside| {
            fs::rename(root, aside).unwrap_or_else(|e| unreachable!("{e}"));
            fs::create_dir(root).unwrap_or_else(|e| unreachable!("{e}"));
        });
    assert!(
        matches!(&entries, Err(error) if error.to_string().contains("was replaced by another directory while it was listed")),
        "{entries:?}"
    );
}
