// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;

use clap::Parser;

use super::super::jit::Jit;
use crate::cli::Cli;

fn parse(extra: &[&str]) -> bool {
    let mut argv = vec!["velnor-runner", "sandbox-cleanup"];
    argv.extend(extra);
    Cli::try_parse_from(argv).is_ok()
}

#[test]
fn the_cleanup_takes_one_reservation_or_all_and_nothing_else() {
    assert!(parse(&["--reservation", "7"]));
    assert!(parse(&["--all"]));
    assert!(!parse(&[]));
    assert!(!parse(&["--reservation", "7", "--all"]));
}

#[test]
fn the_jit_is_read_within_its_bound_and_comes_out_of_the_pipe_once() {
    assert!(Jit::read(&b""[..]).is_err());
    assert!(Jit::read(&vec![b'x'; 70 * 1024][..]).is_err());
    let jit = Jit::read(&b"secret-jit"[..]).expect("a small JIT is accepted");
    let mut reader = jit.pipe().expect("the pipe opens");
    let mut text = String::new();
    assert!(reader.read_to_string(&mut text).is_ok());
    assert_eq!(text, "secret-jit");
    let mut again = String::new();
    assert!(reader.read_to_string(&mut again).is_ok());
    assert_eq!(again, "");
}

#[test]
fn the_process_limit_counts_as_hit_only_after_a_fork_was_refused() {
    use super::super::cgroup::limit_hit;
    assert!(!limit_hit("max 0\n"));
    assert!(limit_hit("max 3\n"));
    assert!(!limit_hit(""));
    assert!(!limit_hit("max x\n"));
}

#[test]
fn trusted_facts_are_opened_without_following_links_and_only_as_a_bounded_regular_file() {
    use super::super::trusted::Header;
    let root = std::env::temp_dir().join(format!("velnor-trusted-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let good = root.join("good.json");
    let facts = include_str!("../../../tests/fixtures/trusted/pull_request.json");
    assert!(std::fs::write(&good, facts).is_ok());
    assert!(Header::read(&good).is_ok());
    let link = root.join("link.json");
    assert!(std::os::unix::fs::symlink(&good, &link).is_ok());
    assert!(Header::read(&link).is_err(), "a symlink was followed");
    assert!(Header::read(&root).is_err(), "a directory was read");
    let large = root.join("large.json");
    assert!(std::fs::write(&large, vec![b' '; 70 * 1024]).is_ok());
    assert!(Header::read(&large).is_err(), "an oversized file was read");
    let _ = std::fs::remove_dir_all(&root);
}
