// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::mbx_reports::SWITCH;
use super::mbx_socket::{SocketDir, run_bounded};
use super::{ResultTest, fixture};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
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
