// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn checkout_never_forwards_credentials_to_redirected_origin()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let destination = root.join("checkout");
    fs::create_dir(&destination)?;
    let target = TcpListener::bind("127.0.0.1:0")?;
    target.set_nonblocking(true)?;
    let target_address = target.local_addr()?;
    let target_stop = Arc::new(AtomicBool::new(false));
    let done = Arc::clone(&target_stop);
    let target_requests = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&target_requests);
    let target_worker = thread::spawn(move || {
        while !done.load(Ordering::Relaxed) {
            match target.accept() {
                Ok((mut stream, _)) => {
                    observed.fetch_add(1, Ordering::Relaxed);
                    drop(response(&mut stream, "200 OK", b""));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("target accept: {error}"),
            }
        }
    });
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let origin = format!("http://{}", listener.local_addr()?);
    let location = format!("http://{target_address}/redirect");
    let worker = thread::spawn(
        move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => return Err(error.into()),
                }
            };
            stream.set_nonblocking(false)?;
            let mut request = [0; 4096];
            let _ = stream.read(&mut request)?;
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )?;
            Ok(())
        },
    );
    assert!(
        checkout(
            &SystemGit,
            &destination,
            "owner/demo",
            &"a".repeat(40),
            DUMMY,
            &origin,
            true
        )
        .is_err()
    );
    worker
        .join()
        .map_err(|_| "redirect server panicked")?
        .map_err(|_| "redirect server failed")?;
    target_stop.store(true, Ordering::Relaxed);
    target_worker.join().map_err(|_| "target server panicked")?;
    assert_eq!(target_requests.load(Ordering::Relaxed), 0);
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{DUMMY}"));
    assert_no_credential(&destination, &encoded)?;
    Ok(())
}

#[test]
fn checkout_git_runs_only_the_supplied_tool_and_exec_path() -> Result<(), Box<dyn std::error::Error>>
{
    use std::{cell::Cell, os::unix::fs::PermissionsExt as _};
    struct FixtureGit {
        binary: PathBuf,
        directory: PathBuf,
        verified: Cell<bool>,
    }
    impl GitTool for FixtureGit {
        fn executable(&self) -> &Path {
            &self.binary
        }
        fn exec_path(&self) -> Option<&Path> {
            Some(&self.directory)
        }
        fn verify(&self) -> Result<(), OrchestratorError> {
            self.verified.set(true);
            Ok(())
        }
    }
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let binary = root.join("git-fixture");
    fs::write(&binary, "#!/bin/sh\nprintf '%s' \"$GIT_EXEC_PATH\"\n")?;
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o500))?;
    let tool = FixtureGit {
        binary,
        directory: root.clone(),
        verified: Cell::new(false),
    };
    let output = git(
        &tool,
        &root,
        &["--version".into()],
        None,
        "https://github.com",
        false,
    )?;
    assert_eq!(output, root.to_string_lossy());
    assert!(tool.verified.get());
    Ok(())
}
