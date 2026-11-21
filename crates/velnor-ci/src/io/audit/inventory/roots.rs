// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::descriptors::{descriptor_shortage, shortage_message};
use super::walk::Budget;

pub(in crate::io::audit) fn capture_roots(
    paths: &BTreeSet<PathBuf>,
    budget: &mut Budget,
) -> io::Result<BTreeSet<PathBuf>> {
    capture_roots_with(paths, budget, &|path| fs::canonicalize(path))
}

fn capture_roots_with(
    paths: &BTreeSet<PathBuf>,
    budget: &mut Budget,
    resolve: &dyn Fn(&Path) -> io::Result<PathBuf>,
) -> io::Result<BTreeSet<PathBuf>> {
    let mut roots = BTreeSet::new();
    for path in paths {
        match resolve(path) {
            Ok(root) => {
                roots.insert(root);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => match descriptor_shortage(&error) {
                Some(errno) => {
                    budget.descriptors = true;
                    return Err(io::Error::other(shortage_message(errno, 0)));
                }
                None => {
                    roots.insert(path.clone());
                }
            },
        }
    }
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failing(errno: i32) -> impl Fn(&Path) -> io::Result<PathBuf> {
        move |_| Err(io::Error::from_raw_os_error(errno))
    }

    fn paths(names: &[&str]) -> BTreeSet<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn a_descriptor_shortage_while_resolving_a_root_fails_instead_of_skipping_it() {
        for (errno, wording) in [
            (libc::EMFILE, "raise the open-file limit"),
            (libc::ENFILE, "system-wide"),
        ] {
            let mut budget = Budget::new(usize::MAX);
            let error = capture_roots_with(&paths(&["/"]), &mut budget, &failing(errno))
                .err()
                .unwrap_or_else(|| unreachable!("a shortage must not skip the root"));
            let message = error.to_string();
            assert!(message.contains(wording), "{message}");
            assert!(budget.descriptors_exhausted());
        }
    }

    #[test]
    fn a_shortage_on_a_later_root_discards_the_roots_resolved_before_it() {
        let mut budget = Budget::new(usize::MAX);
        let resolve = |path: &Path| {
            if path.ends_with("later") {
                Err(io::Error::from_raw_os_error(libc::EMFILE))
            } else {
                Ok(path.to_path_buf())
            }
        };
        let result = capture_roots_with(&paths(&["/", "/later"]), &mut budget, &resolve);
        assert!(result.is_err());
    }

    #[test]
    fn a_root_that_does_not_exist_is_skipped() {
        let mut budget = Budget::new(usize::MAX);
        let roots = capture_roots_with(&paths(&["/", "/absent"]), &mut budget, &|path| {
            if path.ends_with("absent") {
                Err(io::Error::from(io::ErrorKind::NotFound))
            } else {
                Ok(path.to_path_buf())
            }
        })
        .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(roots, paths(&["/"]));
        assert!(!budget.descriptors_exhausted());
    }

    #[test]
    fn a_root_that_cannot_be_resolved_for_another_reason_is_kept_for_the_scan_to_report() {
        let mut budget = Budget::new(usize::MAX);
        let roots = capture_roots_with(&paths(&["/"]), &mut budget, &failing(libc::EACCES))
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(roots, paths(&["/"]));
    }

    #[test]
    fn roots_that_resolve_to_one_directory_are_scanned_once() {
        let mut budget = Budget::new(usize::MAX);
        let roots = capture_roots_with(&paths(&["/a", "/b"]), &mut budget, &|_| Ok("/".into()))
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(roots, paths(&["/"]));
    }
}
