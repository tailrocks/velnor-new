// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use velnor_ci::image_candidate::hashing::sha256_hex;
use velnor_ci::io::image_deploy::checks::verify_staged;

use crate::support::fresh_dir;

fn staged(root: &Path, files: &[(&str, &[u8])]) -> String {
    fs::create_dir_all(root).unwrap_or_else(|e| unreachable!("{e}"));
    files
        .iter()
        .map(|(name, bytes)| {
            fs::write(root.join(name), bytes).unwrap_or_else(|e| unreachable!("{e}"));
            format!("{name}={}", sha256_hex(bytes))
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn refused(dir: &Path, sums: &str) -> String {
    verify_staged(dir, sums)
        .err()
        .map_or_else(String::new, |e| e.to_string())
}

#[test]
fn the_staged_binaries_pass_when_every_hash_matches_and_nothing_else_is_there() {
    let root = fresh_dir("deploy", "verify-ok");
    let sums = staged(&root, &[("api", b"one"), ("worker", b"two")]);
    assert!(verify_staged(&root, &sums).is_ok_and(|report| report.lines().count() == 2));
}

#[test]
fn a_changed_file_an_extra_file_and_a_link_are_refused() {
    let root = fresh_dir("deploy", "verify-changed");
    let sums = staged(&root, &[("api", b"one")]);
    fs::write(root.join("api"), b"other").unwrap_or_else(|e| unreachable!("{e}"));
    assert!(refused(&root, &sums).contains("changed after it was staged"));
    let extra = fresh_dir("deploy", "verify-extra");
    let sums = staged(&extra, &[("api", b"one")]);
    fs::write(extra.join("added"), b"x").unwrap_or_else(|e| unreachable!("{e}"));
    assert!(refused(&extra, &sums).contains("did not stage"));
    let linked = fresh_dir("deploy", "verify-link");
    let sums = staged(&linked, &[("api", b"one")]);
    let secret = linked.with_extension("outside");
    fs::write(&secret, b"one").unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_file(linked.join("api")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(&secret, linked.join("api")).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        refused(&linked, &sums).contains("not a regular file"),
        "a link to a file with the same content is still refused"
    );
}

#[test]
fn a_malformed_or_empty_sum_list_is_refused_before_any_file_is_read() {
    let root = fresh_dir("deploy", "verify-malformed");
    for sums in [
        "",
        "api",
        "api=short",
        "a/b=0000000000000000000000000000000000000000000000000000000000000000",
    ] {
        assert!(verify_staged(&root, sums).is_err(), "{sums}");
    }
}
