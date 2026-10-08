use std::io::Cursor;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::*;

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempDirectory(std::path::PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-process-test-{}-{id}", std::process::id()));
        std::fs::create_dir(&path).expect("unique temporary directory");
        Self(path)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _cleanup = std::fs::remove_dir_all(&self.0);
    }
}

struct TestChild(Option<Child>);

impl TestChild {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn id(&self) -> u32 {
        self.0.as_ref().expect("child is retained").id()
    }

    fn kill_and_reap(mut self) {
        let mut child = self.0.take().expect("child is retained");
        child.kill().expect("test cleanup kills child");
        child.wait().expect("test cleanup reaps child");
    }
}

impl Drop for TestChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _kill = child.kill();
            let _wait = child.wait();
        }
    }
}

#[test]
fn identity_loss_fences_process_group_and_direct_child_signals() {
    let mut command = Command::new("/usr/bin/sleep");
    command.arg("30").process_group(0);
    let child = command.spawn().expect("owned process starts");
    let mut resources = ChildResources {
        child,
        child_identity_pinned: false,
        stdout_reader: None,
        stderr_reader: None,
    };

    resources.signal_group(Signal::TERM);
    resources.kill_direct_child();

    assert!(matches!(resources.child.try_wait(), Ok(None)));
    resources
        .child
        .kill()
        .expect("test still owns the child identity");
    resources.child.wait().expect("test reaps its child");
}

#[test]
fn timeout_kills_pipe_holding_process_group_and_reaps_its_direct_child() {
    let temporary = TempDirectory::new();
    let pid_file = temporary.0.join("pids");
    let script = concat!(
        "trap '' TERM; ",
        "/usr/bin/sleep 30 & descendant=$!; ",
        "printf '%s\\n%s\\n' \"$$\" \"$descendant\" > \"$1\"; ",
        "wait"
    );
    let mut command = Command::new("/bin/sh");
    command.args(["-c", script, "process-test"]);
    command.arg(&pid_file);
    let unrelated = TestChild::new(
        Command::new("/usr/bin/sleep")
            .arg("30")
            .spawn()
            .expect("unrelated process starts"),
    );
    let deadline = Instant::now() + Duration::from_millis(250);

    let error = output_until(&mut command, Some(deadline)).expect_err("deadline expires");
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(std::fs::metadata(&pid_file).is_ok());
    let pids = std::fs::read_to_string(&pid_file).expect("helper wrote child identities");
    let mut pids = pids
        .lines()
        .map(|line| line.parse::<u32>().expect("numeric pid"));
    let direct = pids.next().expect("direct child pid");
    let descendant = pids.next().expect("descendant pid");

    wait_for_process_absence(direct);
    wait_for_process_not_running(descendant);
    assert!(
        process_state(unrelated.id()).is_some_and(|state| state != 'Z'),
        "a process outside the command's process group must survive"
    );
    unrelated.kill_and_reap();
}

#[test]
fn terminating_a_descendant_allows_its_parent_and_the_helper_to_reap_it() {
    let temporary = TempDirectory::new();
    let pid_file = temporary.0.join("pids");
    let script = concat!(
        "trap '' TERM; ",
        "/bin/sh -c 'trap - TERM; exec /usr/bin/sleep 30' & descendant=$!; ",
        "printf '%s\\n%s\\n' \"$$\" \"$descendant\" > \"$1\"; ",
        "wait \"$descendant\""
    );
    let mut command = Command::new("/bin/sh");
    command.args(["-c", script, "process-test"]);
    command.arg(&pid_file);

    let error = output_until(
        &mut command,
        Some(Instant::now() + Duration::from_millis(250)),
    )
    .expect_err("the helper's absolute deadline expires");
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    let pids = std::fs::read_to_string(pid_file).expect("helper wrote child identities");
    let mut pids = pids
        .lines()
        .map(|line| line.parse::<u32>().expect("numeric pid"));

    wait_for_process_absence(pids.next().expect("direct child pid"));
    wait_for_process_absence(pids.next().expect("descendant pid"));
}

#[test]
fn capture_retains_at_most_the_configured_output_limit() {
    let stream = capture_stream(Cursor::new(vec![b'x'; MAX_CAPTURED_OUTPUT_BYTES + 17]))
        .expect("cursor read succeeds");

    assert_eq!(stream.bytes.len(), MAX_CAPTURED_OUTPUT_BYTES);
    assert!(stream.exceeded_limit);
}

#[test]
fn bounded_command_rejects_output_above_the_capture_limit() {
    let mut command = Command::new("/usr/bin/head");
    command.args([
        "-c",
        &(MAX_CAPTURED_OUTPUT_BYTES + 1).to_string(),
        "/dev/zero",
    ]);

    let error = output_until(&mut command, Some(Instant::now() + Duration::from_secs(2)))
        .expect_err("oversized output is rejected");

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
}

fn process_state(pid: u32) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let closing_parenthesis = stat.rfind(')')?;
    stat.get(closing_parenthesis + 1..)?
        .split_whitespace()
        .next()?
        .chars()
        .next()
}

fn wait_for_process_absence(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while process_state(pid).is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(process_state(pid), None, "direct child must be reaped");
}

fn wait_for_process_not_running(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while process_state(pid).is_some_and(|state| state != 'Z') && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        process_state(pid).is_none_or(|state| state == 'Z'),
        "terminated descendant must no longer run"
    );
}
