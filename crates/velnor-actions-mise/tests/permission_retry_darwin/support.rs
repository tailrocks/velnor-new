// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

pub(super) fn run_fixture(
    test_name: &str,
    mode: &str,
    verify_log: impl FnOnce(&str),
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let library = preprovisioned_library()?;
    let log_path = directory.path().join("interpose.log");
    let exact_name = format!("permission_retry_darwin::{test_name}");
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["--exact", &exact_name, "--nocapture"])
        .env("VELNOR_GROUP_PROBE_CHILD", "1")
        .env("VELNOR_GROUP_PROBE_MODE", mode)
        .env("VELNOR_GROUP_PROBE_LOG", &log_path)
        .env("VELNOR_GROUP_PROBE_DIRECTORY", directory.path())
        .env("DYLD_INSERT_LIBRARIES", &library);
    let output = run_bounded_child(command, Duration::from_secs(10))?;
    assert!(
        output.status.success(),
        "child failed: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let log = fs::read_to_string(log_path)?;
    assert!(
        log.contains("interposed pgid=-"),
        "fixture did not activate: {log}"
    );
    verify_log(&log);
    Ok(())
}

#[derive(Deserialize)]
struct ProbeReceipt {
    library_path: PathBuf,
    library_sha256: String,
    source_path: PathBuf,
    source_sha256: String,
    compiler_path: PathBuf,
    compiler_version: String,
    compile_argv: Vec<String>,
}

fn preprovisioned_library() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let receipt_path = std::env::var_os("VELNOR_GROUP_PROBE_RECEIPT")
        .ok_or_else(|| io::Error::other("preprovisioned interposer receipt is missing"))?;
    let receipt_path = PathBuf::from(receipt_path);
    verify_private_artifact(&receipt_path, false, 0o222)?;
    let receipt: ProbeReceipt = serde_json::from_slice(&fs::read(&receipt_path)?)?;
    let library = receipt.library_path;
    assert_eq!(
        receipt_path.parent(),
        library.parent(),
        "receipt and interposer directories differ"
    );
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/group_signal_zero_interpose.c")
        .canonicalize()?;
    let compiler = Path::new("/usr/bin/cc").canonicalize()?;
    let expected_argv = vec![
        "/usr/bin/cc".to_owned(),
        "-dynamiclib".to_owned(),
        "-o".to_owned(),
        library.to_string_lossy().into_owned(),
        source.to_string_lossy().into_owned(),
    ];
    assert!(library.is_absolute(), "interposer path is not absolute");
    assert_eq!(
        library.canonicalize()?,
        library,
        "interposer path is not canonical"
    );
    assert_eq!(receipt.source_path, source, "fixture source path changed");
    assert_eq!(receipt.compiler_path, compiler, "compiler path changed");
    assert!(
        !receipt.compiler_version.trim().is_empty(),
        "compiler version is missing"
    );
    assert_eq!(
        receipt.compile_argv, expected_argv,
        "compile command changed"
    );
    assert_eq!(
        sha256_file(&source)?,
        receipt.source_sha256,
        "fixture source digest changed"
    );
    verify_private_artifact(
        library
            .parent()
            .ok_or_else(|| io::Error::other("interposer parent is missing"))?,
        true,
        0o077,
    )?;
    verify_private_artifact(&library, false, 0o222)?;
    assert_eq!(
        sha256_file(&library)?,
        receipt.library_sha256,
        "interposer digest changed"
    );
    Ok(library)
}

fn verify_private_artifact(path: &Path, directory: bool, forbidden_mode: u32) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let valid_type = if directory {
        metadata.is_dir()
    } else {
        metadata.is_file()
    };
    if !valid_type
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || (!directory && metadata.nlink() != 1)
        || metadata.mode() & forbidden_mode != 0
    {
        return Err(io::Error::other(format!(
            "unsafe preprovisioned artifact: {}",
            path.display()
        )));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> io::Result<String> {
    let digest = Sha256::digest(fs::read(path)?);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        encoded.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    Ok(encoded)
}

pub(super) fn in_fixture_child() -> bool {
    std::env::var_os("VELNOR_GROUP_PROBE_CHILD").is_some()
}

fn run_bounded_child(mut command: Command, timeout: Duration) -> io::Result<Output> {
    command.process_group(0);
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    let mut child = command.spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output(),
            Ok(None) => {}
            Err(error) => {
                let cleanup = terminate_and_reap(&mut child);
                return Err(match cleanup {
                    Ok(()) => io::Error::other(format!("fixture child wait failed: {error}")),
                    Err(cleanup_error) => io::Error::other(format!(
                        "fixture child wait failed: {error}; cleanup failed: {cleanup_error}"
                    )),
                });
            }
        }
        if Instant::now() >= deadline {
            let cleanup = terminate_and_reap(&mut child);
            let output = child.wait_with_output()?;
            cleanup?;
            return Err(io::Error::other(format!(
                "fixture child exceeded {timeout:?}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn terminate_and_reap(child: &mut std::process::Child) -> io::Result<()> {
    let group_kill = if let Ok(raw_pid) = i32::try_from(child.id())
        && let Some(pid) = Pid::from_raw(raw_pid)
    {
        kill_process_group(pid, Signal::KILL)
    } else {
        Err(rustix::io::Errno::INVAL)
    };
    let child_kill = if group_kill.is_err() {
        child.kill()
    } else {
        Ok(())
    };
    let wait = child.wait().map(|_| ());
    wait?;
    child_kill?;
    match group_kill {
        Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
        Err(error) => Err(io::Error::from(error)),
    }
}

pub(super) struct ReleaseOnDrop(pub(super) PathBuf);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        drop(fs::write(&self.0, b"release"));
    }
}

pub(super) fn deadline(
    duration: Duration,
) -> Result<CheckDeadline, velnor_actions_mise::MiseError> {
    CheckDeadline::after(duration)
}

pub(super) fn shell(script: &str) -> Result<IsolatedCommand, velnor_actions_mise::MiseError> {
    IsolatedCommand::repo_task(
        "/bin/sh",
        vec![OsString::from("-c"), OsString::from(script)],
        &[],
    )
}

pub(super) fn shell_with_args(
    script: &str,
    arguments: &[&Path],
) -> Result<IsolatedCommand, velnor_actions_mise::MiseError> {
    let mut args = vec![
        OsString::from("-c"),
        OsString::from(script),
        OsString::from("probe"),
    ];
    args.extend(arguments.iter().map(|path| path.as_os_str().to_owned()));
    IsolatedCommand::repo_task("/bin/sh", args, &[])
}

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> io::Result<Self> {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-mise-permission-retry-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.path));
    }
}
