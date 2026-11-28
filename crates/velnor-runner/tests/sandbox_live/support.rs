// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

pub(crate) const UID: u32 = 5001;
pub(crate) const GID: u32 = 5002;
pub(crate) const HOME: &str = "/home/runner";
pub(crate) const GENERATIONS: &str = "/srv/velnor/gen";
pub(crate) const RUNNER: &str = env!("CARGO_BIN_EXE_velnor-runner");
pub(crate) const TRUSTED: &str = include_str!("../fixtures/trusted/pull_request.json");

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut bytes = u32::try_from(payload.len())
        .unwrap_or(u32::MAX)
        .to_be_bytes()
        .to_vec();
    bytes.extend_from_slice(payload);
    bytes
}

pub(crate) struct Sandbox {
    pub(crate) reservation: u64,
    arguments: Vec<String>,
    frames: Option<Vec<u8>>,
    user: (u32, u32, String),
}

pub(crate) fn require_live_environment() {
    assert!(
        rustix::process::geteuid().is_root()
            && Path::new("/var/lib/velnor/tools").is_dir()
            && Path::new(GENERATIONS).is_dir(),
        "the live sandbox lane needs root, /var/lib/velnor/tools and /srv/velnor/gen; provision it with scripts/velnor-sandbox-live.ts"
    );
}

impl Sandbox {
    pub(crate) fn new(name: &str) -> Self {
        require_live_environment();
        let reservation = 1_000_000
            + name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
            }) % 1_000_000;
        Self {
            reservation,
            arguments: vec!["--machine-floor-gib".to_owned(), "0".to_owned()],
            frames: None,
            user: (UID, GID, HOME.to_owned()),
        }
    }

    pub(crate) fn running_as(mut self, uid: u32, gid: u32, home: &str) -> Self {
        self.user = (uid, gid, home.to_owned());
        self
    }

    pub(crate) fn with_jit(self, jit: &str) -> Self {
        self.with_frames(jit.as_bytes(), TRUSTED.as_bytes())
    }

    pub(crate) fn with_frames(mut self, jit: &[u8], trusted: &[u8]) -> Self {
        self.frames = Some([frame(jit), frame(trusted)].concat());
        self
    }

    pub(crate) fn with(mut self, flag: &str, value: &str) -> Self {
        if let Some(at) = self.arguments.iter().position(|argument| argument == flag) {
            value.clone_into(&mut self.arguments[at + 1]);
        } else {
            self.arguments.push(flag.to_owned());
            self.arguments.push(value.to_owned());
        }
        self
    }

    pub(crate) fn work_dir(&self) -> PathBuf {
        Path::new(GENERATIONS)
            .join("work")
            .join(self.reservation.to_string())
    }

    pub(crate) fn command(&self, command: &[&str]) -> Command {
        let mut process = Command::new(RUNNER);
        process
            .args(["sandbox", "--profile", "job", "--reservation"])
            .arg(self.reservation.to_string())
            .args(["--uid", &self.user.0.to_string()])
            .args(["--gid", &self.user.1.to_string()])
            .args(["--home", &self.user.2])
            .arg("--work-dir")
            .arg(self.work_dir())
            .args(&self.arguments)
            .args(self.frames.iter().map(|_| "--stdin-frames"))
            .arg("--")
            .args(command)
            .stdin(if self.frames.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        process
    }

    pub(crate) fn spawn(&self, command: &[&str]) -> Option<Child> {
        let mut child = self.command(command).spawn().ok()?;
        if let (Some(frames), Some(mut stdin)) = (&self.frames, child.stdin.take()) {
            stdin.write_all(frames).ok()?;
        }
        Some(child)
    }

    pub(crate) fn run(&self, command: &[&str]) -> Output {
        unstarted(
            self.spawn(command)
                .ok_or_else(|| std::io::Error::other("the sandbox did not start"))
                .and_then(Child::wait_with_output),
        )
    }

    pub(crate) fn shell(&self, script: &str) -> Output {
        self.run(&["/bin/sh", "-c", script])
    }
}

pub(crate) fn unstarted(result: std::io::Result<Output>) -> Output {
    result.unwrap_or_else(|error| Output {
        status: ExitStatusExt::from_raw(255 << 8),
        stdout: Vec::new(),
        stderr: format!("the process did not start: {error}").into_bytes(),
    })
}

pub(crate) fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

pub(crate) fn succeeded(output: &Output) -> String {
    assert!(output.status.success(), "{}", text(output));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub(crate) fn failed(output: &Output) -> String {
    assert!(
        !output.status.success(),
        "the command succeeded but had to fail: {}",
        text(output)
    );
    text(output)
}

pub(crate) fn cgroup(sandbox: &Sandbox) -> PathBuf {
    PathBuf::from(format!("/sys/fs/cgroup/velnor/job-{}", sandbox.reservation))
}

pub(crate) fn processes_running(program: &str, argument: &str) -> usize {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            std::fs::read(entry.path().join("cmdline")).is_ok_and(|bytes| {
                let mut parts = bytes.split(|byte| *byte == 0);
                parts.next() == Some(program.as_bytes())
                    && parts.next() == Some(argument.as_bytes())
            })
        })
        .count()
}

pub(crate) fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !condition() {
        if std::time::Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    true
}

pub(crate) fn joined(sandbox: &Sandbox) -> bool {
    wait_until(|| {
        std::fs::read_to_string(cgroup(sandbox).join("cgroup.procs"))
            .is_ok_and(|procs| !procs.trim().is_empty())
    })
}

pub(crate) fn cleanup(sandbox: &Sandbox) -> Output {
    unstarted(
        Command::new(RUNNER)
            .args(["sandbox-cleanup", "--reservation"])
            .arg(sandbox.reservation.to_string())
            .output(),
    )
}

pub(crate) fn run_dir(sandbox: &Sandbox) -> PathBuf {
    PathBuf::from(format!("/run/velnor/jobs/{}", sandbox.reservation))
}

pub(crate) struct Cleaned<'a>(pub(crate) &'a Sandbox);

impl Drop for Cleaned<'_> {
    fn drop(&mut self) {
        let _ = cleanup(self.0);
    }
}

pub(crate) fn nothing_left(sandbox: &Sandbox) -> bool {
    !cgroup(sandbox).exists()
        && !Path::new(&format!("/run/netns/velnor-job-{}", sandbox.reservation)).exists()
        && !sandbox.work_dir().exists()
        && !Path::new(GENERATIONS)
            .join("stage")
            .join(sandbox.reservation.to_string())
            .exists()
}

pub(crate) const BTRFS_IOCTL: &str = "import ctypes, os, struct, sys\nlibc = ctypes.CDLL(None, use_errno=True)\nfd = os.open(sys.argv[1], os.O_RDONLY | os.O_DIRECTORY)\nbuffer = ctypes.create_string_buffer(struct.pack('q4088s', 0, sys.argv[3].encode()))\nstatus = libc.ioctl(fd, ctypes.c_ulong(int(sys.argv[2], 16)), buffer)\nprint('created' if status == 0 else ctypes.get_errno())\n";

pub(crate) fn breach_record(sandbox: &Sandbox) -> String {
    std::fs::read_to_string(run_dir(sandbox).join("breach.json")).unwrap_or_default()
}

pub(crate) fn killed_with(sandbox: &Sandbox, script: &[&str], message: &str, kind: &str) {
    let output = sandbox.run(script);
    let log = text(&output);
    assert_eq!(output.status.code(), Some(125), "{log}");
    assert!(log.contains(message), "{log}");
    assert!(
        !log.contains("survived"),
        "the script outlived the kill: {log}"
    );
    assert_eq!(
        breach_record(sandbox).trim(),
        format!("{{\"breach\":\"{kind}\"}}")
    );
    assert!(
        nothing_left(sandbox),
        "the killed job left files or its cgroup"
    );
    assert!(cleanup(sandbox).status.success());
    assert!(
        !run_dir(sandbox).exists(),
        "the breach record outlived the cleanup"
    );
}
