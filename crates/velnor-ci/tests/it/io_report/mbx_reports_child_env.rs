// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::mbx_reports::SWITCH;
use super::mbx_socket::{SocketDir, fill_backlog, run_bounded};
use super::{Repo, ResultTest, fixture};
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::Command;

#[test]
fn a_stale_socket_is_not_inherited_when_mbx_reports_is_off() -> ResultTest {
    let repo = fixture("mbx-stale-switch-off")?;
    let bin = repo.root().join("fake-bin");
    fs::create_dir_all(&bin)?;
    let cargo = bin.join("cargo");
    fs::write(
        &cargo,
        "#!/bin/sh\nif [ -z \"${MBX_SOCKET+x}\" ]; then printf unset > \"$FAKE_CHILD_ENV\"; else printf set > \"$FAKE_CHILD_ENV\"; fi\n",
    )?;
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755))?;
    let sockets = SocketDir::for_repo(&repo)?;
    let socket = sockets.socket("cache-agent.sock");
    drop(UnixListener::bind(&socket)?);
    let observed = repo.root().join("child-env");
    let output = run_bounded(
        Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["run", "--", cargo.to_str().ok_or("path")?, "build"])
            .current_dir(repo.root())
            .env("RUNNER_TEMP", repo.root())
            .env_remove(SWITCH)
            .env("MBX_SOCKET", &socket)
            .env("FAKE_CHILD_ENV", &observed),
    )?;
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&observed)?, "unset");
    Ok(())
}

const RECORD_CHILD_ENV: &str = "#!/bin/sh\nprintf '%s|%s' \"${MBX_SOCKET-unset}\" \"${MBX_STATS_REPORT-unset}\" > \"$FAKE_CHILD_ENV\"\n";

fn observe(repo: &Repo, socket: &Path) -> Result<(String, String), Box<dyn std::error::Error>> {
    let bin = repo.root().join("fake-bin");
    fs::create_dir_all(&bin)?;
    let cargo = bin.join("cargo");
    fs::write(&cargo, RECORD_CHILD_ENV)?;
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755))?;
    let observed = repo.root().join("child-env");
    let output = run_bounded(
        Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["run", "--", cargo.to_str().ok_or("path")?, "build"])
            .current_dir(repo.root())
            .env("RUNNER_TEMP", repo.root())
            .env(SWITCH, "true")
            .env_remove("MBX_STATS_REPORT")
            .env("MBX_SOCKET", socket)
            .env("FAKE_CHILD_ENV", &observed),
    )?;
    assert!(output.status.success(), "{output:?}");
    let seen = fs::read_to_string(&observed)?;
    let (socket, report) = seen.split_once('|').ok_or("child env")?;
    Ok((socket.to_owned(), report.to_owned()))
}

fn raw_connect_is_refused(socket: &Path) -> bool {
    use nix::errno::Errno;
    use nix::sys::socket::{AddressFamily, SockFlag, SockType, UnixAddr, connect, socket as open};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    let Ok(address) = UnixAddr::new(socket) else {
        return false;
    };
    let Ok(endpoint) = open(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::empty(),
        None,
    ) else {
        return false;
    };
    let endpoint = UnixStream::from(endpoint);
    endpoint.set_nonblocking(true).is_ok()
        && connect(endpoint.as_raw_fd(), &address) == Err(Errno::ECONNREFUSED)
}

#[test]
fn a_live_session_with_a_full_queue_keeps_its_socket() -> ResultTest {
    let repo = fixture("mbx-lease-live-full")?;
    let sockets = SocketDir::for_repo(&repo)?;
    let socket = sockets.socket("cache-agent.sock");
    let _listener = UnixListener::bind(&socket)?;
    let lease = File::create(sockets.socket("owner.lock"))?;
    lease.lock()?;
    let _clients = fill_backlog(&socket)?;
    if cfg!(target_os = "macos") {
        assert!(raw_connect_is_refused(&socket));
    }
    let (kept, report) = observe(&repo, &socket)?;
    assert_eq!(Path::new(&kept), socket);
    assert_eq!(report, "unset");
    Ok(())
}

#[test]
fn a_dead_session_whose_lease_is_free_loses_its_socket_and_gets_a_report() -> ResultTest {
    let repo = fixture("mbx-lease-dead-free")?;
    let sockets = SocketDir::for_repo(&repo)?;
    let socket = sockets.socket("cache-agent.sock");
    drop(UnixListener::bind(&socket)?);
    drop(File::create(sockets.socket("owner.lock"))?);
    assert!(raw_connect_is_refused(&socket));
    let (kept, report) = observe(&repo, &socket)?;
    assert_eq!(kept, "unset");
    assert!(
        Path::new(&report).starts_with(repo.root().join("velnor-mbx-reports")),
        "{report}"
    );
    Ok(())
}

#[test]
fn a_live_enclosing_session_still_hides_the_reports_switch_from_a_foreign_child() -> ResultTest {
    let repo = fixture("mbx-live-switch")?;
    let bin = repo.root().join("fake-bin");
    fs::create_dir_all(&bin)?;
    let cargo = bin.join("cargo");
    fs::write(
        &cargo,
        "#!/bin/sh\nprintf '%s' \"${VELNOR_MBX_REPORTS-unset}\" > \"$FAKE_CHILD_ENV\"\n",
    )?;
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755))?;
    let sockets = SocketDir::for_repo(&repo)?;
    let socket = sockets.socket("cache-agent.sock");
    let _listener = UnixListener::bind(&socket)?;
    let observed = repo.root().join("child-env");
    let output = run_bounded(
        Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["run", "--", cargo.to_str().ok_or("path")?, "build"])
            .current_dir(repo.root())
            .env("RUNNER_TEMP", repo.root())
            .env(SWITCH, "true")
            .env("MBX_SOCKET", &socket)
            .env("FAKE_CHILD_ENV", &observed),
    )?;
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&observed)?, "unset");
    Ok(())
}

#[test]
fn a_test_process_never_inherits_the_job_switch_that_opens_mbx_report_sessions() {
    assert_ne!(
        std::env::var(SWITCH).as_deref(),
        Ok("true"),
        "a velnor-ci that a test starts would open sessions in the job's RUNNER_TEMP that no MBX run reports, and Check cache waste fails on them"
    );
}
