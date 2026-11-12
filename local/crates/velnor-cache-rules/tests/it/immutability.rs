// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{AlreadyExists, Event, reserve};

use crate::support::{MAIN, PR7, VERSION, access, entry, id, pr, push};

type Reserved = Result<(), AlreadyExists>;

#[test]
fn rule_4_same_key_and_version_in_the_same_scope_cannot_be_created_again() {
    let entries = [
        entry(MAIN, "d-1", VERSION, 1),
        entry(PR7, "d-2", VERSION, 2),
    ];
    let refused = |scope, key| Err(AlreadyExists(id(scope, key, VERSION)));
    let table: [(&str, Event, &str, &str, Reserved); 7] = [
        (
            "push saving an existing key",
            push("main"),
            "d-1",
            VERSION,
            refused(MAIN, "d-1"),
        ),
        (
            "pr saving its own earlier key",
            pr(7, "main"),
            "d-2",
            VERSION,
            refused(PR7, "d-2"),
        ),
        (
            "same key, other version",
            push("main"),
            "d-1",
            "zstd",
            Ok(()),
        ),
        ("same key, other branch", push("b"), "d-1", VERSION, Ok(())),
        (
            "pr saving a key of the default branch",
            pr(7, "main"),
            "d-1",
            VERSION,
            Ok(()),
        ),
        (
            "another pr saving a key of pr 7",
            pr(8, "main"),
            "d-2",
            VERSION,
            Ok(()),
        ),
        (
            "a prefix of an existing key",
            push("main"),
            "d",
            VERSION,
            Ok(()),
        ),
    ];
    for (name, event, key, version, expected) in table {
        let reserved = reserve(&entries, &access(&event), key, version);
        assert_eq!(reserved, expected, "{name}");
    }
}
