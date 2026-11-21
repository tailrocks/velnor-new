// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::symlink;

use super::*;

fn arena(name: &str) -> PathBuf {
    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-exhaustion-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    base
}

fn failing_at(root: &Path, leaf: &'static str, errno: i32) -> (io::Error, Budget) {
    let mut budget = Budget::new(usize::MAX);
    let error = scan_with(
        root,
        &BTreeSet::new(),
        &mut budget,
        &Probe {
            before_open: &|directory| {
                if directory.ends_with(leaf) {
                    Err(io::Error::from_raw_os_error(errno))
                } else {
                    Ok(())
                }
            },
            ..Probe::default()
        },
    )
    .err()
    .unwrap_or_else(|| unreachable!("the scan reached {leaf} and must fail"));
    (error, budget)
}

fn nested(name: &str) -> PathBuf {
    let root = arena(name);
    fs::create_dir_all(root.join("a/b/c")).unwrap_or_else(|e| unreachable!("{e}"));
    root
}

#[test]
fn a_per_process_descriptor_shortage_names_the_depth_and_asks_to_raise_the_limit() {
    let root = nested("emfile");
    let (error, budget) = failing_at(&root, "c", libc::EMFILE);
    let message = error.to_string();
    assert!(
        message.contains("deeper than 3 levels")
            && message.contains("exceeds the open-file limit (")
            && message.contains("raise the open-file limit"),
        "{message}"
    );
    assert!(budget.descriptors_exhausted());
}

#[test]
fn a_system_wide_descriptor_shortage_is_not_blamed_on_the_per_process_limit() {
    let root = nested("enfile");
    let (error, budget) = failing_at(&root, "c", libc::ENFILE);
    let message = error.to_string();
    assert!(message.contains("system-wide"), "{message}");
    assert!(message.contains("3 levels"), "{message}");
    assert!(
        !message.contains("raise the open-file limit")
            && !message.contains("exceeds the open-file limit"),
        "{message}"
    );
    assert!(budget.descriptors_exhausted());
}

#[test]
fn other_open_errors_are_not_reported_as_descriptor_shortages() {
    let root = nested("other");
    let (error, budget) = failing_at(&root, "c", libc::EACCES);
    assert_eq!(error.raw_os_error(), Some(libc::EACCES), "{error}");
    assert!(!budget.descriptors_exhausted());
}

#[test]
fn depth_follows_a_directory_link_that_leaves_the_scanned_root() {
    let base = arena("external");
    let root = base.join("root");
    fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(base.join("outside/a/b/c")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(base.join("outside/a"), root.join("link")).unwrap_or_else(|e| unreachable!("{e}"));
    let (error, _) = failing_at(&root, "c", libc::EMFILE);
    let message = error.to_string();
    assert!(message.contains("deeper than 3 levels"), "{message}");
}
