// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;

use clap::Parser;

use super::super::cgroup::{counted, memory_limit};
use super::super::disk::Usage;
use super::super::frames::Delivery;
use super::super::jit::Jit;
use super::super::run_dir::write_at;
use super::super::watch::Breach;
use crate::cli::Cli;

fn parse(extra: &[&str]) -> bool {
    let mut argv = vec!["velnor-runner", "sandbox-cleanup"];
    argv.extend(extra);
    Cli::try_parse_from(argv).is_ok()
}

fn frame(length: usize, payload: &[u8]) -> Vec<u8> {
    let mut bytes = u32::try_from(length)
        .unwrap_or(u32::MAX)
        .to_be_bytes()
        .to_vec();
    bytes.extend_from_slice(payload);
    bytes
}

fn two(jit: &[u8], trusted: &[u8]) -> Vec<u8> {
    let mut bytes = frame(jit.len(), jit);
    bytes.extend(frame(trusted.len(), trusted));
    bytes
}

#[test]
fn the_cleanup_takes_one_reservation_or_all_and_nothing_else() {
    assert!(parse(&["--reservation", "7"]));
    assert!(parse(&["--all"]));
    assert!(!parse(&[]));
    assert!(!parse(&["--reservation", "7", "--all"]));
}

#[test]
fn the_stdin_frames_come_jit_first_then_trusted_facts_each_within_64_kib() {
    let delivered = Delivery::read(&two(b"jit-secret", b"{}")[..]).expect("two small frames");
    assert_eq!(&delivered.jit[..], b"jit-secret");
    assert_eq!(&delivered.trusted[..], b"{}");
    let largest = vec![b'x'; 64 * 1024];
    assert!(Delivery::read(&two(&largest, &largest)[..]).is_ok());
    let over = vec![b'x'; 64 * 1024 + 1];
    assert!(
        Delivery::read(&two(&over, b"{}")[..]).is_err(),
        "a JIT over 64 KiB"
    );
    assert!(
        Delivery::read(&two(b"jit", &over)[..]).is_err(),
        "facts over 64 KiB"
    );
    assert!(
        Delivery::read(&two(b"", b"{}")[..]).is_err(),
        "an empty JIT"
    );
    assert!(
        Delivery::read(&two(b"jit", b"")[..]).is_err(),
        "empty facts"
    );
    assert!(
        Delivery::read(&frame(9, b"short")[..]).is_err(),
        "a frame shorter than its length"
    );
    assert!(
        Delivery::read(&frame(3, b"jit")[..]).is_err(),
        "a delivery without the trusted frame"
    );
    assert!(Delivery::read(&b""[..]).is_err());
}

#[test]
fn the_jit_comes_out_of_the_pipe_once() {
    let delivered = Delivery::read(&two(b"secret-jit", b"{}")[..]).expect("a delivery");
    let mut reader = Jit(delivered.jit).pipe().expect("the pipe opens");
    let mut text = String::new();
    assert!(reader.read_to_string(&mut text).is_ok());
    assert_eq!(text, "secret-jit");
    let mut again = String::new();
    assert!(reader.read_to_string(&mut again).is_ok());
    assert_eq!(again, "");
}

#[test]
fn the_process_limit_counts_as_hit_only_after_a_fork_was_refused() {
    assert!(!counted("max 0\n", "max"));
    assert!(counted("low 0\nmax 3\n", "max"));
    assert!(!counted("", "max"));
    assert!(!counted("max x\n", "max"));
    assert!(!counted("maxed 3\n", "max"));
}

#[test]
fn a_memory_kill_counts_only_after_the_kernel_killed_a_process() {
    assert!(!counted("oom 0\noom_kill 0\n", "oom_kill"));
    assert!(counted("oom 2\noom_kill 1\noom_group_kill 1\n", "oom_kill"));
    assert!(!counted("oom_group_kill 1\n", "oom_kill"));
}

#[test]
fn a_job_gets_its_share_of_ninety_percent_of_the_memory() {
    assert_eq!(memory_limit(1_000_000, 1), 921_600_000);
    assert_eq!(memory_limit(1_000_000, 4), 230_400_000);
    assert_eq!(memory_limit(1_000_000, 0), 921_600_000);
}

#[test]
fn a_breach_names_its_kind_for_the_host_record() {
    let kinds = [
        (Breach::Disk(Usage::default()), "disk"),
        (Breach::ForeignDevice, "foreign_device"),
        (Breach::MachineFull, "machine_full"),
        (Breach::Processes, "processes"),
        (Breach::Memory, "memory"),
        (Breach::Killed, "killed"),
    ];
    for (breach, kind) in kinds {
        assert_eq!(breach.kind(), kind);
    }
}

#[test]
fn the_trusted_facts_are_created_new_and_never_through_a_planted_link() {
    let root = std::env::temp_dir().join(format!("velnor-run-dir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    assert!(std::fs::create_dir_all(&root).is_ok());
    let directory = root.join("job");
    assert!(std::fs::create_dir(&directory).is_ok());
    assert!(write_at(&directory, "trusted.json", b"facts").is_ok());
    assert_eq!(
        std::fs::read(directory.join("trusted.json"))
            .ok()
            .as_deref(),
        Some(&b"facts"[..])
    );
    assert_eq!(
        std::os::unix::fs::MetadataExt::mode(
            &std::fs::metadata(directory.join("trusted.json")).expect("metadata")
        ) & 0o777,
        0o600
    );
    assert!(
        write_at(&directory, "trusted.json", b"again").is_err(),
        "an existing file was reused"
    );
    let victim = root.join("victim");
    assert!(std::fs::write(&victim, b"keep").is_ok());
    assert!(std::os::unix::fs::symlink(&victim, directory.join("planted")).is_ok());
    assert!(
        write_at(&directory, "planted", b"facts").is_err(),
        "a planted link was followed"
    );
    assert_eq!(std::fs::read(&victim).ok().as_deref(), Some(&b"keep"[..]));
    let linked = root.join("linked");
    assert!(std::os::unix::fs::symlink(&directory, &linked).is_ok());
    assert!(
        write_at(&linked, "other.json", b"facts").is_err(),
        "a linked directory was entered"
    );
    let _ = std::fs::remove_dir_all(&root);
}
