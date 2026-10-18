//! Product-binary checks for bounded controller helper requests.

use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HOST: &str = env!("CARGO_BIN_EXE_velnor-host");
const FLAG: &str = "--internal-control-request";
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct OwnedState(PathBuf);

impl OwnedState {
    fn new() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = PathBuf::from(format!(
            "/tmp/velnor-control-helper-{}-{nonce:x}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        fs::write(path.join("drain"), TOKEN)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn listener(&self) -> Result<UnixListener, Box<dyn Error>> {
        let path = self.0.join("controller.sock");
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        Ok(listener)
    }
}

impl Drop for OwnedState {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn read_request(stream: &mut UnixStream) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut prefix = [0; 4];
    stream.read_exact(&mut prefix)?;
    let mut bytes = vec![0; u32::from_be_bytes(prefix) as usize];
    stream.read_exact(&mut bytes)?;
    Ok(())
}

fn accept(listener: &UnixListener) -> Result<UnixStream, Box<dyn Error + Send + Sync>> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                return Ok(stream);
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => return Err(Box::new(error)),
        }
    }
}

fn run_control(
    state: &Path,
    timeout_ms: u64,
    extra: bool,
) -> Result<std::process::Output, Box<dyn Error>> {
    let mut request = serde_json::json!({
        "operation": "drain",
        "state": state,
        "timeout_ms": timeout_ms,
    });
    if extra {
        request["unexpected"] = true.into();
    }
    let bytes = serde_json::to_vec(&request)?;
    let mut child = Command::new(HOST)
        .arg(FLAG)
        .arg(timeout_ms.to_string())
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let Some(mut input) = child.stdin.take() else {
        return Err("helper stdin missing".into());
    };
    input.write_all(&bytes)?;
    drop(input);
    Ok(child.wait_with_output()?)
}

fn wait_bounded(
    child: &mut std::process::Child,
    bound: Duration,
) -> Result<std::process::ExitStatus, Box<dyn Error>> {
    let deadline = Instant::now() + bound;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let _ = child.wait()?;
            return Err("The host did not finish before the time limit.".into());
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn start_open_stdin(timeout_arg: &str) -> Result<std::process::Child, Box<dyn Error>> {
    Ok(Command::new(HOST)
        .arg(FLAG)
        .arg(timeout_arg)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?)
}

#[test]
fn product_binary_kills_and_reaps_a_stalled_control_child_by_deadline() -> Result<(), Box<dyn Error>>
{
    let state = OwnedState::new()?;
    let listener = state.listener()?;
    let (release_peer, wait_for_release) = mpsc::channel();
    let peer = thread::spawn(move || -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut stream = accept(&listener)?;
        read_request(&mut stream)?;
        wait_for_release.recv_timeout(Duration::from_secs(15))?;
        Ok(())
    });

    let output = run_control(state.path(), 8_000, false)?;
    let released = release_peer.send(()).is_ok();
    let peer_result = peer
        .join()
        .map_err(|_| "peer panicked")?
        .map_err(|error| error.to_string());
    peer_result?;

    assert!(output.status.success());
    assert_eq!(output.stdout, b"false\n");
    assert!(released, "The host did not send the request to the peer.");
    assert_eq!(fs::read(state.path().join("drain"))?, TOKEN.as_bytes());
    Ok(())
}

#[test]
fn helper_keeps_offline_missing_journal_as_non_proof() -> Result<(), Box<dyn Error>> {
    let state = OwnedState::new()?;
    let output = run_control(state.path(), 2_000, false)?;
    assert!(output.status.success());
    assert_eq!(output.stdout, b"false\n");
    assert!(!state.path().join("launch.db").exists());
    Ok(())
}

#[test]
fn helper_rejects_unknown_request_fields_without_a_boolean_result() -> Result<(), Box<dyn Error>> {
    let state = OwnedState::new()?;
    let output = run_control(state.path(), 2_000, true)?;
    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    Ok(())
}

#[test]
fn parent_deadline_bounds_stdin_held_open() -> Result<(), Box<dyn Error>> {
    let mut child = start_open_stdin("120")?;
    let started = Instant::now();
    let status = wait_bounded(&mut child, Duration::from_secs(2))?;
    let output = child.wait_with_output()?;
    assert!(!status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[test]
fn parent_deadline_bounds_partial_stdin() -> Result<(), Box<dyn Error>> {
    let mut child = start_open_stdin("120")?;
    let Some(stdin) = child.stdin.as_mut() else {
        return Err("helper stdin missing".into());
    };
    stdin.write_all(br#"{"operation":"drain""#)?;
    let status = wait_bounded(&mut child, Duration::from_secs(2))?;
    let output = child.wait_with_output()?;
    assert!(!status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    Ok(())
}

#[test]
fn malformed_timeout_is_rejected_before_reading_stdin() -> Result<(), Box<dyn Error>> {
    let mut child = start_open_stdin("0120")?;
    let status = wait_bounded(&mut child, Duration::from_secs(3))?;
    let output = child.wait_with_output()?;
    assert!(!status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    Ok(())
}

#[test]
fn child_and_boolean_output_share_the_input_deadline() -> Result<(), Box<dyn Error>> {
    let state = OwnedState::new()?;
    let listener = state.listener()?;
    let (release_peer, wait_for_release) = mpsc::channel();
    let peer = thread::spawn(move || -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut stream = accept(&listener)?;
        read_request(&mut stream)?;
        wait_for_release.recv_timeout(Duration::from_secs(5))?;
        Ok(())
    });
    let request = serde_json::to_vec(&serde_json::json!({
        "operation": "drain",
        "state": state.path(),
        "timeout_ms": 2_000,
    }))?;
    let mut child = Command::new(HOST)
        .arg(FLAG)
        .arg("2000")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let Some(mut input) = child.stdin.take() else {
        return Err("helper stdin missing".into());
    };
    let writer = thread::spawn(move || -> Result<(), Box<dyn Error + Send + Sync>> {
        thread::sleep(Duration::from_secs(1));
        input.write_all(&request)?;
        drop(input);
        Ok(())
    });
    let started = Instant::now();
    let status = wait_bounded(&mut child, Duration::from_secs(5))?;
    let output = child.wait_with_output()?;
    writer
        .join()
        .map_err(|_| "input writer panicked")?
        .map_err(|error| error.to_string())?;
    let released = release_peer.send(()).is_ok();
    peer.join()
        .map_err(|_| "peer panicked")?
        .map_err(|error| error.to_string())?;
    assert!(status.success());
    assert_eq!(output.stdout, b"false\n");
    assert!(
        released,
        "The child process did not finish before the time limit."
    );
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(3_400),
        "Input and child operations took {elapsed:?}. The time limit was 2,000 ms."
    );
    Ok(())
}
