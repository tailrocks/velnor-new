// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Child;

use super::support::{
    Sandbox, TRUSTED, cleanup, failed, run_dir, succeeded, text, unstarted, wait_until,
};

const MIRRORS: &str = "/var/lib/velnor/git/github.com/live";

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
fn the_jit_reaches_the_command_once_through_a_pipe_and_leaves_no_file() {
    let sandbox = Sandbox::new("jit").with_jit("jit-secret\n");
    let read = succeeded(&sandbox.run(&["/bin/cat"]));
    assert_eq!(read, "jit-secret\n");
    let again = succeeded(&sandbox.shell("cat; cat /proc/$$/fd/0; echo end"));
    assert_eq!(
        again, "jit-secret\nend\n",
        "a second read found the JIT again"
    );
    let descriptors = succeeded(&sandbox.shell("ls /proc/self/fd | sort -n | tr '\\n' ' '"));
    assert_eq!(descriptors.trim(), "0 1 2 3", "{descriptors}");
    let init = succeeded(&sandbox.shell(
        "cat /proc/1/fd/0 2>&1; cat /proc/1/environ 2>&1; cat /proc/1/cmdline | tr '\\0' ' '",
    ));
    assert!(!init.contains("jit-secret"), "{init}");
    assert!(
        !std::path::Path::new("/run/velnor-live-jit").exists(),
        "a JIT file exists"
    );
}

#[test]
fn a_frame_larger_than_64_kib_or_empty_is_refused_before_anything_starts() {
    let largest = "x".repeat(64 * 1024);
    let fits = Sandbox::new("jit-fits").with_jit(&largest);
    assert!(fits.run(&["/bin/true"]).status.success());
    for (name, jit, trusted) in [
        ("jit-large", "x".repeat(64 * 1024 + 1), TRUSTED.to_owned()),
        ("trusted-large", "jit".to_owned(), "x".repeat(64 * 1024 + 1)),
        ("jit-empty", String::new(), TRUSTED.to_owned()),
    ] {
        let sandbox = Sandbox::new(name).with_frames(jit.as_bytes(), trusted.as_bytes());
        let refusal = failed(&sandbox.run(&["/bin/true"]));
        assert!(
            refusal.contains("empty or larger than"),
            "{name}: {refusal}"
        );
        assert!(!sandbox.work_dir().exists(), "{name}");
    }
}

#[test]
fn trusted_facts_land_in_a_root_only_tmpfs_file_the_job_cannot_reach_and_a_broken_frame_starts_nothing()
 {
    let sandbox = Sandbox::new("trusted").with_jit("jit");
    let mut child = sandbox.spawn(&["/bin/sleep", "608"]);
    let file = run_dir(&sandbox).join("trusted.json");
    assert!(
        wait_until(|| file.exists()),
        "the trusted facts were never written"
    );
    let metadata = std::fs::metadata(&file).expect("the trusted facts file");
    assert_eq!(metadata.mode() & 0o7777, 0o600);
    assert_eq!(metadata.uid(), 0);
    assert_eq!(
        std::fs::read_to_string(&file).ok().as_deref(),
        Some(TRUSTED)
    );
    let filesystem = unstarted(
        std::process::Command::new("stat")
            .args(["-f", "-c", "%T"])
            .arg(&file)
            .output(),
    );
    assert_eq!(String::from_utf8_lossy(&filesystem.stdout).trim(), "tmpfs");
    let _ = child.as_mut().map(Child::kill);
    let _ = child.as_mut().map(Child::wait);
    assert!(cleanup(&sandbox).status.success());
    assert!(!run_dir(&sandbox).exists());

    let described = sandbox.run(&["/bin/true"]);
    assert!(
        text(&described)
            .contains("repository tailrocks/velnor-lab(42) sha 0123456789ab event pull_request"),
        "{}",
        text(&described)
    );
    assert!(!run_dir(&sandbox).exists(), "the run dir outlived the job");
    let broken = Sandbox::new("trusted-broken").with_frames(b"jit", b"{\"repository\": 1}");
    let refused = failed(&broken.run(&["/bin/true"]));
    assert!(refused.contains("trusted facts"), "{refused}");
    assert!(!broken.work_dir().exists());
    assert!(!run_dir(&broken).exists());
}

#[test]
fn the_job_cannot_see_the_run_directory_of_the_trusted_facts() {
    let sandbox = Sandbox::new("run-hidden").with_jit("jit");
    let seen = succeeded(&sandbox.shell("ls /run; ls /run/velnor 2>&1; echo end"));
    assert_eq!(
        seen,
        "ls: cannot access '/run/velnor': No such file or directory\nend\n"
    );
}
