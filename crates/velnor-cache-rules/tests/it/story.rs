// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::support::{found, rows, saved};

#[test]
fn story_push_saves_pr_restores_saves_and_restores_again() {
    let mut repository = Vec::new();
    for row in rows(
        "restore; push:main; deps-main-1; deps-; -
         save; push:main; deps-main-1; ; -
         restore; pr:7:main; deps-pr7-1; deps-; main|deps-main-1
         save; pr:7:main; deps-pr7-1; ; -
         restore; pr:7:main; deps-pr7-1; deps-; #7|deps-pr7-1
         save; pr:7:main; deps-pr7-1; ; #7|deps-pr7-1|v1
         restore; pr:8:main; deps-pr7-1; ; -
         restore; pr:8:main; deps-pr8-1; deps-; main|deps-main-1
         save; pr:8:main; deps-pr7-1; ; -
         restore; push:main; deps-pr7-1; deps-pr; -",
    ) {
        let outcome = match row[0] {
            "restore" => found(&repository, row[1], row[2], row[3], "v1"),
            _ => saved(&mut repository, row[1], row[2]),
        };
        assert_eq!(outcome, row[4], "{row:?}");
    }
}
