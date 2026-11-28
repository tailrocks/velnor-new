// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use super::identity::{Databases, is_sole_identity, sudo_refused};
use super::job_dirs::removal_verdict;
use super::parse::{self, Entry};
use super::user::USER_HOME;
use super::{ANCESTORS, CLOSED_DIRECTORIES, DIRECTORY, Layout, OPEN_DIRECTORIES, classify};
use crate::error::RustRunnerFailure;
use crate::rust_runner::GuestUser;

fn entry(uid: u32, mode: u32, kind: &str) -> Entry {
    Entry {
        uid,
        gid: 0,
        mode,
        kind: kind.to_owned(),
    }
}

fn sound() -> HashMap<String, Entry> {
    let mut entries = HashMap::new();
    for path in ANCESTORS.iter().chain(&OPEN_DIRECTORIES) {
        entries.insert((*path).to_owned(), entry(0, 0o755, DIRECTORY));
    }
    for path in CLOSED_DIRECTORIES {
        entries.insert(path.to_owned(), entry(0, 0o700, DIRECTORY));
    }
    entries
}

#[test]
fn a_root_owned_layout_is_sound() {
    assert_eq!(classify(&sound()), Layout::Sound);
}

#[test]
fn an_ancestor_that_anyone_else_can_change_is_unsafe() {
    for (ancestor, replacement) in [
        ("/var/lib", entry(1000, 0o755, DIRECTORY)),
        ("/var/lib", entry(0, 0o775, DIRECTORY)),
        ("/var", entry(0, 0o757, DIRECTORY)),
        ("/", entry(0, 0o777, DIRECTORY)),
        ("/var", entry(0, 0o755, "symbolic link")),
    ] {
        let mut entries = sound();
        entries.insert(ancestor.to_owned(), replacement.clone());
        assert_eq!(
            classify(&entries),
            Layout::Unsafe,
            "{ancestor} {replacement:?}"
        );
    }
    let mut missing = sound();
    missing.remove("/var/lib");
    assert_eq!(classify(&missing), Layout::Unsafe);
}

#[test]
fn a_store_directory_that_is_wrong_is_repaired() {
    let wrong = [
        ("/var/lib/velnor/tools", entry(0, 0o775, DIRECTORY)),
        ("/var/lib/velnor/locks", entry(0, 0o755, DIRECTORY)),
        ("/var/lib/velnor/aside", entry(0, 0o750, DIRECTORY)),
    ];
    for (path, replacement) in wrong {
        let mut entries = sound();
        entries.insert(path.to_owned(), replacement.clone());
        assert_eq!(classify(&entries), Layout::Repair, "{path} {replacement:?}");
    }
    for path in OPEN_DIRECTORIES.iter().chain(&CLOSED_DIRECTORIES) {
        let mut entries = sound();
        entries.remove(*path);
        assert_eq!(classify(&entries), Layout::Repair, "{path} missing");
    }
}

#[test]
fn a_store_entry_that_is_not_a_directory_is_unsafe_and_never_repaired() {
    for path in OPEN_DIRECTORIES.iter().chain(&CLOSED_DIRECTORIES) {
        for kind in ["symbolic link", "regular file"] {
            for uid in [0, 1000] {
                let mut entries = sound();
                entries.insert((*path).to_owned(), entry(uid, 0o755, kind));
                assert_eq!(classify(&entries), Layout::Unsafe, "{path} {kind} {uid}");
            }
        }
    }
}

#[test]
fn a_store_directory_owned_by_another_user_is_refused_by_name_and_never_re_owned() {
    for path in OPEN_DIRECTORIES.iter().chain(&CLOSED_DIRECTORIES) {
        let mut entries = sound();
        entries.insert((*path).to_owned(), entry(1000, 0o755, DIRECTORY));
        assert_eq!(
            classify(&entries),
            Layout::Foreign((*path).to_owned()),
            "{path}"
        );
    }
}

#[test]
fn stat_lines_keep_a_file_kind_with_a_space() {
    let parsed = parse::entries(
        "/var/lib/velnor/bin/velnor-runner 0 0 755 regular file\n/var/lib/velnor 0 0 755 directory\nstat: cannot statx '/x': No such file or directory\n",
    );
    assert!(parsed["/var/lib/velnor/bin/velnor-runner"].is("regular file", 0o755));
    assert!(parsed["/var/lib/velnor"].is("directory", 0o755));
    assert_eq!(parsed.len(), 2);
}

#[test]
fn digests_are_strict() {
    let sum = "a".repeat(64);
    assert_eq!(
        parse::digest(&format!("{sum}  /var/lib/velnor/bin/velnor-runner\n")),
        Some(sum.clone())
    );
    assert_eq!(parse::digest(&format!("{}  x", "a".repeat(63))), None);
    assert_eq!(parse::digest(&format!("{}  x", "g".repeat(64))), None);
    assert_eq!(parse::digest("sha256sum: x: No such file"), None);
}

#[test]
fn command_lines_drop_blank_rows() {
    assert_eq!(parse::lines("btrfs\n\n btrfs \n"), vec!["btrfs", "btrfs"]);
    assert_eq!(parse::lines("\n"), Vec::<&str>::new());
}

#[test]
fn a_removal_that_fails_or_leaves_a_path_wedges_the_slot_and_an_orb_failure_does_not() {
    assert_eq!(removal_verdict(Some(0)), Ok(()));
    for code in [70, 71] {
        assert_eq!(
            removal_verdict(Some(code)),
            Err(RustRunnerFailure::JobDirsWedged),
            "{code}"
        );
    }
    for code in [Some(1), Some(127), Some(255), None] {
        assert_eq!(
            removal_verdict(code),
            Err(RustRunnerFailure::JobFilesLeft),
            "{code:?}"
        );
    }
}

#[test]
fn only_a_nologin_job_user_with_nonzero_ids_parses() {
    let line = |uid: &str, gid: &str, shell: &str| {
        format!("velnor-job:x:{uid}:{gid}::{USER_HOME}:{shell}\n")
    };
    let user = GuestUser::parse(&line("500", "501", "/usr/sbin/nologin"))
        .map(|user| (user.uid(), user.gid(), user.home().to_owned()));
    assert_eq!(user, Some((500, 501, USER_HOME.to_owned())));
    for bad in [
        line("0", "1000", "/usr/sbin/nologin"),
        line("1000", "0", "/usr/sbin/nologin"),
        line("500", "501", "/bin/bash"),
        format!("other:x:500:501::{USER_HOME}:/usr/sbin/nologin\n"),
        format!(
            "{}root:x:0:0:root:/root:/bin/bash\n",
            line("500", "501", "/usr/sbin/nologin")
        ),
    ] {
        assert_eq!(GuestUser::parse(&bad), None, "{bad}");
    }
}

#[cfg(feature = "test-support")]
#[test]
fn the_test_support_constructor_equals_a_parsed_user() {
    assert_eq!(
        GuestUser::parse(&format!(
            "velnor-job:x:500:501::{USER_HOME}:/usr/sbin/nologin\n"
        )),
        Some(GuestUser::for_tests(500, 501, USER_HOME))
    );
}

fn job() -> GuestUser {
    GuestUser::for_tests(777, 778, USER_HOME)
}

#[test]
fn only_a_sole_uid_and_gid_make_a_sole_identity() {
    fn found<'a>(account: &'a str, passwd: &'a str, group: &'a str) -> Databases<'a> {
        Databases {
            uid_lookup: account,
            all_accounts: passwd,
            gid_lookup: "velnor-job:x:778:\n",
            all_groups: group,
        }
    }
    let account = format!("velnor-job:x:777:778::{USER_HOME}:/usr/sbin/nologin\n");
    let base = format!("root:x:0:0:r:/root:/bin/sh\n{account}");
    assert!(is_sole_identity(
        &job(),
        &found(&account, &base, "root:x:0:\nvelnor-job:x:778:\n")
    ));
    assert!(!is_sole_identity(
        &job(),
        &found(
            &account,
            &format!("{base}ubuntu:x:777:1::/:/bin/sh\n"),
            "velnor-job:x:778:\n"
        )
    ));
    assert!(!is_sole_identity(
        &job(),
        &found(
            &account,
            &format!("{base}ubuntu:x:1:778::/:/bin/sh\n"),
            "velnor-job:x:778:\n"
        )
    ));
    assert!(!is_sole_identity(
        &job(),
        &found(&account, &base, "sudo:x:778:\nvelnor-job:x:778:\n")
    ));
    assert!(!is_sole_identity(
        &job(),
        &found(&account, "garbage\n", "velnor-job:x:778:\n")
    ));
}

#[test]
fn a_reserved_uid_or_gid_is_refused_although_nothing_else_holds_it() {
    for (uid, gid) in [(27, 778), (777, 27), (777, 0), (0, 778)] {
        let user = GuestUser::for_tests(uid, gid, USER_HOME);
        let account = format!("velnor-job:x:{uid}:{gid}::{USER_HOME}:/usr/sbin/nologin\n");
        let group = format!("velnor-job:x:{gid}:\n");
        let found = Databases {
            uid_lookup: &account,
            all_accounts: &account,
            gid_lookup: &group,
            all_groups: &group,
        };
        assert!(!is_sole_identity(&user, &found), "{uid} {gid}");
    }
}

#[test]
fn only_the_exact_denial_of_sudo_counts() {
    assert!(sudo_refused(
        "User velnor-job is not allowed to run sudo on box.\n"
    ));
    for report in [
        "",
        "User velnor-job may run the following commands on box:\n    (ALL) NOPASSWD: ALL\n",
        "User velnor-job is not allowed to run sudo on box.\nextra\n",
        "User other is not allowed to run sudo on box.\n",
        "sudo: command not found\n",
    ] {
        assert!(!sudo_refused(report), "{report}");
    }
}
