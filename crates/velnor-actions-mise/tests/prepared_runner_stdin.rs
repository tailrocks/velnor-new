// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    io::{BufRead as _, Write as _},
    os::{fd::OwnedFd, unix::net::UnixStream},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn prepared_runner_keeps_control_stream_private() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("VELNOR_PREPARED_STDIN_CHILD").is_some() {
        let mut command = Command::new("/bin/sh");
        command.env_clear().args([
            "-c",
            "if IFS= read -r x; then exit 17; fi; (if IFS= read -r x; then exit 18; fi) || exit; printf eof",
        ]);
        let deadline = velnor_actions_mise::CheckDeadline::after(Duration::from_secs(3))?;
        let (output, _) = velnor_actions_mise::command::run_prepared_quiescent_until(
            &mut command,
            1024,
            deadline,
        )?;
        assert!(output.success, "a launched process read the control stream");
        assert_eq!(output.stdout, b"eof");
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        assert_eq!(line, "fixture-control-sentinel\n");
        println!("control stream retained");
        return Ok(());
    }

    let (mut peer, child_socket) = UnixStream::pair()?;
    let child_fd: OwnedFd = child_socket.into();
    let mut child = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "prepared_runner_stdin::prepared_runner_keeps_control_stream_private",
            "--nocapture",
        ])
        .env("VELNOR_PREPARED_STDIN_CHILD", "1")
        .stdin(Stdio::from(child_fd))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    peer.write_all(b"fixture-control-sentinel\n")?;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Err("prepared runner failed to return before the deadline".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8(output.stdout)?.contains("control stream retained"));
    Ok(())
}
