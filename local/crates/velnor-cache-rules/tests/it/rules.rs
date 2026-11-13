// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{
    ENTRY_LIFETIME_SECONDS, REPOSITORY_LIMIT_BYTES, Timestamp, access_for, evict,
};

use crate::support::{entries, event, found, list, reserved, rows, short};

fn access_table(table: &str) {
    for row in rows(table) {
        let access = access_for(&event(row[0]), row[1]);
        let read: Vec<String> = access.read.iter().map(short).collect();
        assert_eq!(read, list(row[2]), "{row:?}");
        assert_eq!(short(&access.write), row[3], "{row:?}");
    }
}

fn found_table(table: &str) {
    for row in rows(table) {
        assert_eq!(
            found(&entries(row[1]), row[0], row[2], row[3], row[4]),
            row[5],
            "{row:?}"
        );
    }
}

fn evict_table(table: &str) {
    for row in rows(table) {
        let ids = evict(
            &entries(row[0]),
            Timestamp(10_000_000),
            row[1].parse().unwrap_or_default(),
        );
        let evicted: Vec<String> = ids
            .iter()
            .map(|id| format!("{}|{}", short(&id.scope), id.key))
            .collect();
        assert_eq!(evicted, list(row[2]), "{row:?}");
    }
}

#[test]
fn rule_1_read_scopes_in_lookup_order() {
    access_table(
        "push:main; main; main; main
         push:b; main; b main; b
         pr:7:main; main; #7 main; #7
         pr:7:feature-a; main; #7 feature-a main; #7
         pr:12:trunk; trunk; #12 trunk; #12",
    );
}

#[test]
fn rule_1_sibling_child_and_other_pull_request_scopes_are_not_readable() {
    found_table(
        "push:b; b|k||1; k; -; v1; b|k
         push:b; main|k||1; k; -; v1; main|k
         push:b; c|k||1; k; -; v1; -
         push:main; b|k||1; k; -; v1; -
         push:b; #7|k||1; k; -; v1; -
         pr:7:main; #7|k||1; k; -; v1; #7|k
         pr:8:main; #7|k||1; k; -; v1; -
         pr:7:main; b|k||1; k; -; v1; -
         pr:7:feature-a; feature-a|k||1; k; -; v1; feature-a|k
         pr:7:feature-a; z|k||1; k; -; v1; -
         pr:7:feature-a; main|k||9 feature-a|k||8 #7|k||7; k; -; v1; #7|k
         pr:7:feature-a; main|k||9 feature-a|k||8; k; -; v1; feature-a|k",
    );
}

#[test]
fn rule_2_write_scope_by_event() {
    access_table(
        "push:feature/login; main; feature/login main; feature/login
         pr:42:feature-a; main; #42 feature-a main; #42
         pr:8:main; main; #8 main; #8",
    );
}

#[test]
fn rule_3_matching_by_key_prefix_restore_key_version_and_scope() {
    found_table(
        "push:main; main|d-1||1 main|d-1-x||9; d-1; -; v1; main|d-1
         push:main; main|d-1||1 main|d-2||5 main|d-3||3; d-; -; v1; main|d-2
         push:main; main|n-f-old||1 main|n-new||9; n-f-; n-; v1; main|n-f-old
         push:main; main|n-f-a||1 main|n-b||9; zzz; n-f- n-; v1; main|n-f-a
         push:main; main|n-b||9; zzz; n-f- n-; v1; main|n-b
         push:main; main|n-b||5 main|n-c||8 main|n-a||2; zzz; n-; v1; main|n-c
         push:main; main|n-b||1 main|n-b-x||9; zzz; n-b; v1; main|n-b
         push:main; main|linux-n-1||1; zzz; n-; v1; -
         push:main; main|d-1||1; other; cargo-; v1; -",
    );
    found_table(
        "push:main; main|d-1|zstd|5 main|d-2|gzip|9; d-1; -; zstd; main|d-1
         push:main; main|d-1|zstd|5 main|d-2|gzip|9; d-1; -; other; -
         push:main; main|d-1|zstd|5 main|d-2|gzip|9; d-2; -; zstd; -
         push:main; main|d-1|zstd|5 main|d-2|gzip|9; d-; -; zstd; main|d-1",
    );
    found_table(
        "pr:7:main; main|n-f-d5ea||1 #7|n-f-0000||2; n-f-d5ea; n-f-; v1; #7|n-f-0000
         pr:7:main; main|n-f-d5ea||1 #7|n-f-0000||2; n-f-d5ea; -; v1; main|n-f-d5ea",
    );
}

#[test]
fn rule_4_same_key_and_version_in_the_same_scope_cannot_be_created_again() {
    for row in rows(
        "push:main; d-1; v1; main|d-1|v1
         pr:7:main; d-2; v1; #7|d-2|v1
         push:main; d-1; zstd; -
         push:b; d-1; v1; -
         pr:7:main; d-1; v1; -
         pr:8:main; d-2; v1; -
         push:main; d; v1; -",
    ) {
        let saved = entries("main|d-1||1 #7|d-2||2");
        assert_eq!(reserved(&saved, row[0], row[1], row[2]), row[3], "{row:?}");
    }
}

#[test]
fn rule_5_expiry_limit_and_least_recently_used_order() {
    evict_table(
        "main|a||0|1|10000000; 10000000000; -
         main|a||0|1|9395200; 10000000000; -
         main|a||0|1|9395199; 10000000000; main|a
         main|a||0|1|10000005; 10000000000; -",
    );
    evict_table(
        "main|a||0|4000000000|9999997 main|b||0|4000000000|9999998 main|c||0|1999999999|9999999; 10000000000; -
         main|a||0|4000000000|9999997 main|b||0|4000000000|9999998 main|c||0|2000000000|9999999; 10000000000; main|a
         main|a||0|4000000000|9999997 main|b||0|4000000000|9999998 main|c||0|4000000000|9999999; 10000000000; main|a",
    );
    assert_eq!(
        (ENTRY_LIFETIME_SECONDS, REPOSITORY_LIMIT_BYTES),
        (604_800, 10_000_000_000)
    );
}

#[test]
fn rule_5_least_recently_used_entries_go_first_until_the_total_fits() {
    evict_table(
        "main|b||0|60|20 main|a||0|60|10 main|c||0|60|10000000; 100; main|a main|b
         main|used||1|60|10000000 main|idle||500|60|9999991; 100; main|idle
         #7|pr||0|1|9999950 main|m||0|1|9999990; 2; #7|pr
         main|old||0|90|9395194 main|b||0|60|9999980 main|c||0|60|10000000; 130; main|old
         main|young||9|60|10000000 main|old||3|60|10000000; 100; main|old
         main|b||0|1|30 main|a||0|1|20; 0; main|a main|b
         -; 100; -",
    );
}

#[test]
fn rule_6_tags_and_merge_groups_by_event() {
    access_table(
        "tag:v1.0.0; main; t:v1.0.0 main; t:v1.0.0
         queue:q7; main; q7 main; q7",
    );
    found_table(
        "tag:a; t:a|k||1; k; -; v1; t:a|k
         tag:b; t:a|k||1; k; -; v1; -
         tag:b; main|k||1; k; -; v1; main|k
         queue:q8; main|k||1; k; -; v1; main|k
         queue:q8; q7|k||1; k; -; v1; -
         queue:q8; #7|k||1; k; -; v1; -",
    );
}
