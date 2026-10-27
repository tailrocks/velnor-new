// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{MAX_PAGES, PAGE_SIZE};
use super::page_finished;

#[test]
fn history_enumeration_rejects_an_incomplete_last_page() {
    for (case, length, page, expected) in [
        ("empty page", 0, 1, Some(true)),
        ("short final page", PAGE_SIZE - 1, MAX_PAGES, Some(true)),
        ("full nonterminal page", PAGE_SIZE, 1, Some(false)),
        ("full final page", PAGE_SIZE, MAX_PAGES, None),
        ("oversized page", PAGE_SIZE + 1, 1, None),
        ("zero page", 0, 0, None),
        ("out of bounds page", 0, MAX_PAGES + 1, None),
    ] {
        let observed = page_finished(length, page);
        match expected {
            Some(complete) => assert_eq!(observed.expect(case), complete, "{case}"),
            None => assert!(observed.is_err(), "{case}"),
        }
    }
}
