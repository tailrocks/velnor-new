// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn a_walk_root_that_is_a_regular_file_fails_the_scan_as_not_a_directory() {
    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-root-file-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    let root = base.join("file");
    fs::write(&root, b"").unwrap_or_else(|e| unreachable!("{e}"));
    let result = scan(&root, &BTreeSet::new(), &mut Budget::new(usize::MAX));
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    match result {
        Err(error) => {
            assert_eq!(error.kind(), io::ErrorKind::NotADirectory, "{error}");
            assert!(
                !error
                    .to_string()
                    .contains("changed while the audit scanned"),
                "a file root was reported as a race to retry: {error}"
            );
        }
        Ok(scanned) => panic!("a file root was scanned: {:?}", scanned.entries),
    }
}
