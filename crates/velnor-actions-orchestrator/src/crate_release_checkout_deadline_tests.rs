// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

struct DelayedGit {
    executable: PathBuf,
}

impl GitTool for DelayedGit {
    fn executable(&self) -> &Path {
        &self.executable
    }
    fn exec_path(&self) -> Option<&Path> {
        None
    }
    fn verify(&self) -> Result<(), OrchestratorError> {
        Ok(())
    }
}

#[test]
fn checkout_steps_share_one_deadline() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt as _;
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let source = root.join("source");
    let owner = root.join("owner");
    let destination = root.join("checkout");
    fs::create_dir(&source)?;
    fs::create_dir(&owner)?;
    fs::create_dir(&destination)?;
    let (sha, _) = seed_remote(&root, &source, &owner)?;
    let binary = root.join("delayed-git");
    fs::write(&binary, "#!/bin/sh\nsleep 1.1\nexec /usr/bin/git \"$@\"\n")?;
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o500))?;
    let (origin, stop, count, worker) = server(root.clone())?;
    let result = checkout(
        &DelayedGit { executable: binary },
        &destination,
        "owner/demo",
        &sha,
        DUMMY,
        CheckoutContext {
            origin: &origin,
            test_http: true,
            deadline: velnor_actions_mise::CheckDeadline::after(Duration::from_secs(2))?,
        },
    );
    stop.store(true, Ordering::Relaxed);
    worker.join().map_err(|_| "server panicked")?;
    assert!(result.is_err());
    assert_eq!(count.load(Ordering::Relaxed), 0);
    Ok(())
}

#[test]
fn stalled_git_fetch_is_terminated_before_return() -> Result<(), Box<dyn std::error::Error>> {
    use std::net::Shutdown;
    use std::time::Instant;
    if let Some(origin) = std::env::var_os("VELNOR_STALLED_FETCH_ORIGIN") {
        let temp = tempfile::tempdir()?;
        let target = temp.path().canonicalize()?;
        let result = checkout(
            &SystemGit,
            &target,
            "owner/demo",
            &"a".repeat(40),
            DUMMY,
            CheckoutContext {
                origin: origin.to_str().ok_or("invalid origin")?,
                test_http: true,
                deadline: velnor_actions_mise::CheckDeadline::after(Duration::from_secs(3))?,
            },
        );
        assert!(result.is_err());
        return Ok(());
    }
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let mut child = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "crate_release_checkout::tests::deadline_tests::stalled_git_fetch_is_terminated_before_return",
        ])
        .env(
            "VELNOR_STALLED_FETCH_ORIGIN",
            format!("http://{}", listener.local_addr()?),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let limit = Instant::now() + Duration::from_secs(10);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < limit =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                drop(child.kill());
                drop(child.wait());
                return Err(error.into());
            }
        }
    };
    let mut request = [0; 8192];
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(8)))?;
    let mut headers = Vec::new();
    while !headers.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let count = stream.read(&mut request)?;
        assert!(count > 0);
        headers.extend_from_slice(&request[..count]);
        assert!(headers.len() <= 65536);
    }
    let closed = loop {
        match stream.read(&mut request) {
            Ok(0) => break true,
            Ok(_) => {}
            Err(_) => break false,
        }
    };
    drop(stream.shutdown(Shutdown::Both));
    if !closed {
        drop(child.kill());
    }
    let status = child.wait()?;
    assert!(
        closed,
        "stalled Git fetch remained connected past its deadline"
    );
    assert!(status.success());
    Ok(())
}
