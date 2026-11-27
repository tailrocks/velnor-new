// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::support::{Sandbox, failed, succeeded, text};

const MIRRORS: &str = "/var/lib/velnor/git/github.com/live";
const TRUSTED: &str = include_str!("../fixtures/trusted/pull_request.json");

fn plant(path: &str) {
    let directory = Path::new(path);
    assert!(std::fs::create_dir_all(directory).is_ok());
    assert!(std::fs::write(directory.join("HEAD"), "ref: refs/heads/main\n").is_ok());
}

#[test]
fn a_job_reads_only_the_mirror_it_was_given_and_cannot_write_it() {
    plant(&format!("{MIRRORS}/own/1.git"));
    plant(&format!("{MIRRORS}/other/2.git"));
    let own = format!("{MIRRORS}/own/1.git");
    let sandbox = Sandbox::new("mirror").with("--store-read-only", &own);
    let read = succeeded(&sandbox.shell(&format!("cat {own}/HEAD")));
    assert_eq!(read, "ref: refs/heads/main\n");
    let write = failed(&sandbox.shell(&format!("touch {own}/x 2>&1")));
    assert!(write.contains("Read-only file system"), "{write}");
    let sibling = failed(&sandbox.shell(&format!("ls {MIRRORS}/other 2>&1")));
    assert!(sibling.contains("No such file or directory"), "{sibling}");
    let listing = succeeded(&sandbox.shell(&format!("ls {MIRRORS}")));
    assert_eq!(listing.trim(), "own", "{listing}");
    let _ = std::fs::remove_dir_all("/var/lib/velnor/git/github.com/live");
}

#[test]
fn a_scope_outside_the_cache_and_git_stores_is_refused_before_anything_starts() {
    let sandbox = Sandbox::new("bad-scope").with("--store-read-only", "/var/lib/velnor/tools/key");
    let refusal = failed(&sandbox.shell("true"));
    assert!(
        refusal.contains("not a directory under the cache or git store"),
        "{refusal}"
    );
    assert!(!sandbox.work_dir().exists());
}

#[test]
fn the_jit_file_is_standard_input_and_no_other_descriptor_leaks() {
    let jit = Path::new("/run/velnor-live-jit");
    assert!(std::fs::write(jit, "jit-secret\n").is_ok());
    let sandbox = Sandbox::new("jit").with("--jit", "/run/velnor-live-jit");
    let read = succeeded(&sandbox.run(&["/bin/cat"]));
    assert_eq!(read, "jit-secret\n");
    let descriptors = succeeded(&sandbox.shell("ls /proc/self/fd | sort -n | tr '\\n' ' '"));
    assert_eq!(descriptors.trim(), "0 1 2 3", "{descriptors}");
    let process = succeeded(&sandbox.shell("cat /proc/1/environ | tr '\\0' '\\n'"));
    assert!(!process.contains("jit-secret"), "{process}");
    let _ = std::fs::remove_file(jit);
}

#[test]
fn trusted_facts_of_the_host_contract_start_the_job_and_a_broken_file_does_not() {
    let good = Path::new("/run/velnor-live-trusted.json");
    assert!(std::fs::write(good, TRUSTED).is_ok());
    let sandbox = Sandbox::new("trusted").with("--trusted", "/run/velnor-live-trusted.json");
    let ran = sandbox.run(&["/bin/true"]);
    assert!(ran.status.success(), "{}", text(&ran));
    assert!(
        text(&ran)
            .contains("repository tailrocks/velnor-lab(42) sha 0123456789ab event pull_request"),
        "{}",
        text(&ran)
    );
    assert!(std::fs::write(good, "{\"repository\": 1}").is_ok());
    let refused = failed(&sandbox.run(&["/bin/true"]));
    assert!(refused.contains("trusted facts"), "{refused}");
    assert!(!sandbox.work_dir().exists());
    let _ = std::fs::remove_file(good);
}
