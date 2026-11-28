// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_runner_host::{HostError, RustRunnerFailure};

use super::support::Fixture;

type Check = Result<(), Box<dyn Error>>;

const PASSWD: &str = "root:x:0:0:root:/root:/bin/bash\nubuntu:x:501:501::/home/ubuntu:/bin/bash\n";
const GROUP: &str = "root:x:0:\nsudo:x:27:ubuntu\nubuntu:x:501:\n";
const HOME: &str = concat!("/home", "/velnor-job");
const GOOD: &str = "velnor-job:x:777:778::/home/velnor-job:/usr/sbin/nologin\n";
const GOOD_GROUP: &str = "velnor-job:x:778:\n";

fn databases(fixture: &Fixture, passwd: &str, group: &str) -> Check {
    std::fs::write(fixture.guest("etc/passwd"), format!("{PASSWD}{passwd}"))?;
    std::fs::write(fixture.guest("etc/group"), format!("{group}{GROUP}"))?;
    Ok(())
}

#[test]
fn rust_runner_creates_the_job_user_once_and_returns_it() -> Check {
    let fixture = Fixture::ready()?;
    let expected = (500, 500, HOME);
    for _ in 0..2 {
        let user = fixture.launcher().ensure_machine()?;
        assert_eq!((user.uid(), user.gid(), user.home()), expected);
    }
    assert_eq!(fixture.commands("useradd")?, 1);
    let accounts = std::fs::read_to_string(fixture.guest("etc/passwd"))?;
    assert_eq!(
        accounts
            .lines()
            .filter(|line| line.starts_with("velnor-job:"))
            .count(),
        1
    );
    let line = fixture
        .log()?
        .into_iter()
        .find(|line| line.contains(" useradd "))
        .ok_or("no useradd")?;
    assert!(
        line.ends_with(&format!(
            "useradd --system --user-group --no-create-home --home-dir {HOME} --shell /usr/sbin/nologin velnor-job"
        )),
        "{line}"
    );
    Ok(())
}

#[test]
fn rust_runner_keeps_an_existing_sound_job_user() -> Check {
    let fixture = Fixture::ready()?;
    databases(&fixture, GOOD, GOOD_GROUP)?;
    let user = fixture.launcher().ensure_machine()?;
    assert_eq!((user.uid(), user.gid(), user.home()), (777, 778, HOME));
    assert_eq!(fixture.commands("useradd")?, 0);
    Ok(())
}

fn refused_guest_user(fixture: &Fixture) -> bool {
    fixture.launcher().ensure_machine().err()
        == Some(HostError::RustRunner(RustRunnerFailure::GuestUser))
        && !fixture.guest("var/lib/velnor/bin/velnor-runner").exists()
}

type Account = (&'static str, &'static str, &'static str);

fn unsound_accounts() -> [Account; 14] {
    [
        (
            "uid 0",
            "velnor-job:x:0:778::/home/velnor-job:/usr/sbin/nologin\n",
            GOOD_GROUP,
        ),
        (
            "uid 0 with a non-root gid",
            "velnor-job:x:0:1000::/root:/usr/sbin/nologin\n",
            "velnor-job:x:1000:\n",
        ),
        (
            "gid 0",
            "velnor-job:x:777:0::/home/velnor-job:/usr/sbin/nologin\n",
            "velnor-job:x:0:\n",
        ),
        (
            "a login shell",
            "velnor-job:x:777:778::/home/velnor-job:/bin/bash\n",
            GOOD_GROUP,
        ),
        (
            "a relative home",
            "velnor-job:x:777:778::home:/usr/sbin/nologin\n",
            GOOD_GROUP,
        ),
        (
            "the uid of the default user",
            "velnor-job:x:501:778::/home/velnor-job:/usr/sbin/nologin\n",
            GOOD_GROUP,
        ),
        (
            "the gid of the sudo group, listed before it",
            "velnor-job:x:777:27::/home/velnor-job:/usr/sbin/nologin\n",
            "velnor-job:x:27:\n",
        ),
        (
            "the gid of the default user's group, listed before it",
            "velnor-job:x:777:501::/home/velnor-job:/usr/sbin/nologin\n",
            "velnor-job:x:501:\n",
        ),
        ("a member in its group", GOOD, "velnor-job:x:778:ubuntu\n"),
        (
            "a second account on its uid",
            "velnor-job:x:777:778::/home/velnor-job:/usr/sbin/nologin\nother:x:777:999::/:/usr/sbin/nologin\n",
            GOOD_GROUP,
        ),
        (
            "a second account with its primary gid",
            "velnor-job:x:777:778::/home/velnor-job:/usr/sbin/nologin\nother:x:888:778::/:/usr/sbin/nologin\n",
            GOOD_GROUP,
        ),
        (
            "a second group on its gid",
            GOOD,
            "velnor-job:x:778:\nother:x:778:\n",
        ),
        (
            "a second group named like it",
            GOOD,
            "velnor-job:x:778:\nvelnor-job:x:779:\n",
        ),
        ("no group of its own", GOOD, ""),
    ]
}

#[test]
fn rust_runner_refuses_a_job_user_that_could_do_more_than_run_jobs() -> Check {
    let settings: [(&str, &str); 6] = [
        ("an extra group id", "FAKE_GUEST_GROUPS=500 27\n"),
        ("sudo", "FAKE_GUEST_GROUP_NAMES=velnor-job sudo\n"),
        ("adm", "FAKE_GUEST_GROUP_NAMES=velnor-job adm\n"),
        ("orbstack", "FAKE_GUEST_GROUP_NAMES=velnor-job orbstack\n"),
        ("another primary group", "FAKE_GUEST_GROUP_NAMES=sudo\n"),
        (
            "a sudo report with a failing status",
            "FAKE_GUEST_SUDO_EXIT=1\n",
        ),
    ];
    let mut accepted = Vec::new();
    for (name, passwd, group) in unsound_accounts() {
        let fixture = Fixture::ready()?;
        databases(&fixture, passwd, group)?;
        if !refused_guest_user(&fixture) {
            accepted.push(name);
        }
    }
    for (name, setting) in settings {
        let fixture = Fixture::ready()?;
        fixture.configure(setting)?;
        if !refused_guest_user(&fixture) {
            accepted.push(name);
        }
    }
    assert!(accepted.is_empty(), "accepted: {accepted:?}");
    Ok(())
}

#[test]
fn rust_runner_refuses_a_job_user_with_any_sudo_privilege() -> Check {
    let rules: [(&str, &str); 7] = [
        ("its name", "velnor-job ALL=(ALL) NOPASSWD:ALL"),
        ("its uid", "#777 ALL=(ALL) NOPASSWD:ALL"),
        ("its gid", "%#778 ALL=(ALL) NOPASSWD:ALL"),
        ("its group", "%velnor-job ALL=(ALL) NOPASSWD:ALL"),
        ("everyone", "ALL ALL=(ALL) NOPASSWD:ALL"),
        ("a list that holds it", "other,#777 ALL=(ALL) NOPASSWD:ALL"),
        ("a command only", "velnor-job ALL=(root) /usr/bin/id"),
    ];
    let mut accepted = Vec::new();
    for path in ["etc/sudoers.d/90-job", "etc/sudoers"] {
        for (name, rule) in rules {
            let fixture = Fixture::ready()?;
            databases(&fixture, GOOD, GOOD_GROUP)?;
            std::fs::write(fixture.guest(path), format!("# job\n{rule}\n"))?;
            if !refused_guest_user(&fixture) {
                accepted.push(format!("{name} in {path}"));
            }
        }
    }
    assert!(accepted.is_empty(), "accepted: {accepted:?}");
    let other = Fixture::ready()?;
    databases(&other, GOOD, GOOD_GROUP)?;
    std::fs::write(
        other.guest("etc/sudoers.d/90-other"),
        "# note\n#include /etc/other\nubuntu ALL=(ALL) NOPASSWD:ALL\n%sudo ALL=(ALL) ALL\n#778 ALL=(ALL) ALL\n",
    )?;
    other.launcher().ensure_machine()?;
    Ok(())
}
