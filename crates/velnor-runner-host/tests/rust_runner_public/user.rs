// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_runner_host::{HostError, RustRunnerFailure};

use super::support::Fixture;

type Check = Result<(), Box<dyn Error>>;

const ROOT: &str = "root:x:0:0:root:/root:/bin/bash\n";
const SOUND: &str = "velnor-job:x:777:778::/home/velnor-job:/usr/sbin/nologin\n";

fn passwd(fixture: &Fixture, text: &str) -> Check {
    std::fs::write(fixture.guest("etc/passwd"), format!("{ROOT}{text}"))?;
    Ok(())
}

#[test]
fn rust_runner_creates_the_job_user_once_and_returns_it() -> Check {
    let fixture = Fixture::ready()?;
    let expected = (500, 500, "/home/velnor-job");
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
        line.ends_with(
            "useradd --system --user-group --no-create-home --home-dir /home/velnor-job --shell /usr/sbin/nologin velnor-job"
        ),
        "{line}"
    );
    Ok(())
}

#[test]
fn rust_runner_keeps_an_existing_sound_job_user() -> Check {
    let fixture = Fixture::ready()?;
    passwd(&fixture, SOUND)?;
    let user = fixture.launcher().ensure_machine()?;
    assert_eq!(
        (user.uid(), user.gid(), user.home()),
        (777, 778, "/home/velnor-job")
    );
    assert_eq!(fixture.commands("useradd")?, 0);
    Ok(())
}

fn refused_guest_user(fixture: &Fixture) -> bool {
    fixture.launcher().ensure_machine().err()
        == Some(HostError::RustRunner(RustRunnerFailure::GuestUser))
        && !fixture.guest("var/lib/velnor/bin/velnor-runner").exists()
}

#[test]
fn rust_runner_refuses_a_job_user_that_could_do_more_than_run_jobs() -> Check {
    let accounts: [(&str, &str); 5] = [
        (
            "uid 0",
            "velnor-job:x:0:778::/home/velnor-job:/usr/sbin/nologin\n",
        ),
        (
            "uid 0 with a non-root gid",
            "velnor-job:x:0:1000::/root:/usr/sbin/nologin\n",
        ),
        (
            "gid 0",
            "velnor-job:x:777:0::/home/velnor-job:/usr/sbin/nologin\n",
        ),
        (
            "a login shell",
            "velnor-job:x:777:778::/home/velnor-job:/bin/bash\n",
        ),
        (
            "a relative home",
            "velnor-job:x:777:778::home:/usr/sbin/nologin\n",
        ),
    ];
    let settings: [(&str, &str); 5] = [
        ("an extra group id", "FAKE_GUEST_GROUPS=500 27\n"),
        ("sudo", "FAKE_GUEST_GROUP_NAMES=velnor-job sudo\n"),
        ("adm", "FAKE_GUEST_GROUP_NAMES=velnor-job adm\n"),
        ("orbstack", "FAKE_GUEST_GROUP_NAMES=velnor-job orbstack\n"),
        ("another primary group", "FAKE_GUEST_GROUP_NAMES=sudo\n"),
    ];
    let mut accepted = Vec::new();
    for (name, line) in accounts {
        let fixture = Fixture::ready()?;
        passwd(&fixture, line)?;
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
fn rust_runner_refuses_a_job_user_named_in_a_sudoers_file() -> Check {
    let mut accepted = Vec::new();
    for path in ["etc/sudoers.d/90-job", "etc/sudoers"] {
        let fixture = Fixture::ready()?;
        std::fs::write(
            fixture.guest(path),
            "# job\nvelnor-job ALL=(ALL) NOPASSWD:ALL\n",
        )?;
        if !refused_guest_user(&fixture) {
            accepted.push(path);
        }
    }
    assert!(accepted.is_empty(), "accepted: {accepted:?}");
    let other = Fixture::ready()?;
    std::fs::write(
        other.guest("etc/sudoers.d/90-other"),
        "ubuntu ALL=(ALL) NOPASSWD:ALL\n",
    )?;
    other.launcher().ensure_machine()?;
    Ok(())
}
