// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

use nix::sys::resource::{Resource, getrlimit, setrlimit};

use super::descriptors::{deep_links_tree, deep_tree};
use super::walk::DESCRIPTOR_BUDGET;
use super::*;

const CONFINED: &str = "VELNOR_AUDIT_BUDGET_CHILD";

fn confined(body: impl FnOnce()) {
    if std::env::var_os(CONFINED).is_some() {
        return body();
    }
    let name = std::thread::current()
        .name()
        .map(str::to_owned)
        .unwrap_or_else(|| unreachable!("a test runs on a thread named after it"));
    let exe = std::env::current_exe().unwrap_or_else(|e| unreachable!("{e}"));
    let output = Command::new(exe)
        .args(["--exact", &name, "--nocapture"])
        .env(CONFINED, "1")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{name} failed alone:\n{report}");
    assert!(report.contains("1 passed"), "{name} did not run:\n{report}");
}

fn open_descriptors() -> usize {
    fs::read_dir("/dev/fd").map_or(0, Iterator::count)
}

fn scan_within_the_budget(name: &str, build: impl FnOnce(&Path)) -> io::Result<Entries> {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-scan-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    build(&root);
    let (soft, hard) = getrlimit(Resource::RLIMIT_NOFILE).unwrap_or_else(|e| unreachable!("{e}"));
    let allowed = (open_descriptors() + DESCRIPTOR_BUDGET) as u64;
    setrlimit(Resource::RLIMIT_NOFILE, allowed, hard).unwrap_or_else(|e| unreachable!("{e}"));
    let mut budget = Budget::new(usize::MAX);
    let result = scan_with(&root, &BTreeSet::new(), &mut budget, &Probe::default());
    setrlimit(Resource::RLIMIT_NOFILE, soft, hard).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    result
}

#[test]
fn a_deep_tree_with_pending_siblings_is_walked_within_a_small_descriptor_budget() {
    confined(|| {
        let entries = scan_within_the_budget("deep-budget", |root| deep_tree(root, 160, 4))
            .unwrap_or_else(|e| {
                panic!("walk needed more than {DESCRIPTOR_BUDGET} descriptors: {e}")
            });
        assert_eq!(entries.len(), 1 + 160 * 5, "{}", entries.len());
    });
}

#[test]
fn a_deep_tree_with_deep_link_targets_stays_within_one_descriptor_budget() {
    confined(|| {
        scan_within_the_budget("links-budget", |root| deep_links_tree(root, 120, 56, 200))
            .unwrap_or_else(|e| {
                panic!("audit needed more than {DESCRIPTOR_BUDGET} descriptors: {e}")
            });
    });
}
