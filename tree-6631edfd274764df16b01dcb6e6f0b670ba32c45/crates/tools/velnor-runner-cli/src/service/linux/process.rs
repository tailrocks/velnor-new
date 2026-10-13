use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Output};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid};

mod spawn;

use super::super::{busctl_command, systemctl_command};
use super::{Manager, ManagerOutput};
use spawn::start_output_readers;

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const TERM_CLEANUP_BUDGET: Duration = Duration::from_millis(100);
const KILL_CLEANUP_BUDGET: Duration = Duration::from_millis(250);
pub(super) const MAX_CAPTURED_OUTPUT_BYTES: usize = 128 * 1024;

type ReaderHandle = JoinHandle<io::Result<CapturedStream>>;

#[derive(Default)]
struct CapturedStream {
    bytes: Vec<u8>,
    exceeded_limit: bool,
}

#[derive(Default)]
pub(super) struct Systemctl {
    deadline: Option<Instant>,
}

impl Systemctl {
    pub(super) fn bounded_until(deadline: Instant) -> Self {
        Self {
            deadline: Some(deadline),
        }
    }
}

impl Manager for Systemctl {
    fn systemctl(&mut self, args: &[&str]) -> io::Result<ManagerOutput> {
        let mut command = systemctl_command();
        command.args(args);
        let output = output_until(&mut command, self.deadline)?;
        Ok(ManagerOutput {
            success: output.status.success(),
            stdout: output.stdout,
        })
    }

    fn busctl(&mut self, args: &[&str]) -> io::Result<ManagerOutput> {
        let mut command = busctl_command();
        command.args(args);
        let output = output_until(&mut command, self.deadline)?;
        Ok(ManagerOutput {
            success: output.status.success(),
            stdout: output.stdout,
        })
    }
}

struct ChildResources {
    child: Child,
    child_identity_pinned: bool,
    stdout_reader: Option<ReaderHandle>,
    stderr_reader: Option<ReaderHandle>,
}

impl ChildResources {
    fn readers_finished(&self) -> bool {
        reader_finished(self.stdout_reader.as_ref()) && reader_finished(self.stderr_reader.as_ref())
    }

    fn start_cleanup(mut self, reaper: &Sender<Self>) {
        if !self.child_identity_pinned {
            quarantine(self);
            return;
        }
        self.signal_group(Signal::TERM);
        self.wait_readers_for(TERM_CLEANUP_BUDGET);

        // Keep the direct child unreaped until after SIGKILL. Its PID is the
        // process-group ID, so retaining Child prevents the group ID from
        // being recycled while we signal descendants that may have closed
        // their inherited output pipes.
        self.signal_group(Signal::KILL);
        self.kill_direct_child();
        if !self.wait_quiescent_for(KILL_CLEANUP_BUDGET) {
            defer_reap(self, reaper);
        }
    }

    fn signal_group(&mut self, signal: Signal) {
        if !self.child_identity_pinned {
            return;
        }
        let group = Pid::from_child(&self.child);
        let _signal_result = kill_process_group(group, signal);
    }

    fn kill_direct_child(&mut self) {
        if !self.child_identity_pinned {
            return;
        }
        let _kill_result = self.child.kill();
    }

    fn wait_quiescent_for(&mut self, budget: Duration) -> bool {
        let Some(deadline) = Instant::now().checked_add(budget) else {
            return false;
        };
        loop {
            if self.readers_finished() {
                match self.child.try_wait() {
                    Ok(Some(_)) => {
                        self.child_identity_pinned = false;
                        self.join_readers_discard();
                        return true;
                    }
                    Ok(None) => {}
                    Err(_) => {
                        self.child_identity_pinned = false;
                        return false;
                    }
                }
            }
            if Instant::now() >= deadline {
                return false;
            }
            sleep_until(deadline);
        }
    }

    fn wait_readers_for(&self, budget: Duration) -> bool {
        let Some(deadline) = Instant::now().checked_add(budget) else {
            return false;
        };
        while !self.readers_finished() {
            if Instant::now() >= deadline {
                return false;
            }
            sleep_until(deadline);
        }
        true
    }

    fn join_readers_discard(&mut self) {
        let _stdout = join_reader(self.stdout_reader.take());
        let _stderr = join_reader(self.stderr_reader.take());
    }

    fn reap_until_quiescent(mut self) {
        loop {
            let readers_finished = self.readers_finished();
            if readers_finished {
                match self.child.try_wait() {
                    Ok(Some(_)) => {
                        self.child_identity_pinned = false;
                        self.join_readers_discard();
                        return;
                    }
                    Ok(None) => self.kill_direct_child(),
                    Err(_) => {
                        self.child_identity_pinned = false;
                        quarantine(self);
                        return;
                    }
                }
            }
            thread::sleep(POLL_INTERVAL);
        }
    }
}

static REAPER_SENDER: OnceLock<Mutex<Option<Sender<ChildResources>>>> = OnceLock::new();
static QUARANTINED_CHILDREN: OnceLock<Mutex<Vec<ChildResources>>> = OnceLock::new();

pub(super) fn output_until(command: &mut Command, deadline: Option<Instant>) -> io::Result<Output> {
    let Some(deadline) = deadline else {
        return command.output();
    };
    if Instant::now() >= deadline {
        return Err(timed_out());
    }
    let reaper_sender = reaper_sender()?;

    let mut resources = start_output_readers(command, &reaper_sender)?;

    loop {
        if Instant::now() >= deadline {
            resources.start_cleanup(&reaper_sender);
            return Err(timed_out());
        }
        if resources.readers_finished() {
            match child_exited_without_reaping(&resources.child) {
                Ok(true) if Instant::now() < deadline => match resources.child.try_wait() {
                    Ok(Some(status)) => return collect_output(resources, status),
                    Ok(None) => {
                        resources.child_identity_pinned = false;
                        resources.start_cleanup(&reaper_sender);
                        return Err(io::Error::other(
                            "child exit observation could not be reaped",
                        ));
                    }
                    Err(error) => {
                        resources.child_identity_pinned = false;
                        resources.start_cleanup(&reaper_sender);
                        return Err(error);
                    }
                },
                Ok(true) => {
                    resources.start_cleanup(&reaper_sender);
                    return Err(timed_out());
                }
                Ok(false) => {}
                Err(error) => {
                    if error.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    resources.child_identity_pinned = false;
                    resources.start_cleanup(&reaper_sender);
                    return Err(error);
                }
            }
        }
        sleep_until(deadline);
    }
}

fn child_exited_without_reaping(child: &Child) -> io::Result<bool> {
    waitid(
        WaitId::Pid(Pid::from_child(child)),
        WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
    )
    .map(|status| status.is_some())
    .map_err(io::Error::from)
}

fn terminate_without_readers(
    child: Child,
    kind: io::ErrorKind,
    reaper: &Sender<ChildResources>,
) -> io::Error {
    let resources = ChildResources {
        child,
        child_identity_pinned: true,
        stdout_reader: None,
        stderr_reader: None,
    };
    resources.start_cleanup(reaper);
    io::Error::new(kind, "bounded command output pipe unavailable")
}

fn spawn_reader(
    reader: impl Read + Send + 'static,
    name: &'static str,
) -> io::Result<ReaderHandle> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || capture_stream(reader))
}

fn capture_stream(mut reader: impl Read) -> io::Result<CapturedStream> {
    let mut captured = CapturedStream {
        bytes: Vec::with_capacity(MAX_CAPTURED_OUTPUT_BYTES),
        exceeded_limit: false,
    };
    let mut buffer = [0_u8; 8192];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if read == 0 {
            return Ok(captured);
        }
        let available = MAX_CAPTURED_OUTPUT_BYTES.saturating_sub(captured.bytes.len());
        let retained = read.min(available);
        captured.bytes.extend_from_slice(&buffer[..retained]);
        captured.exceeded_limit |= retained < read;
    }
}

fn collect_output(resources: ChildResources, status: ExitStatus) -> io::Result<Output> {
    let stdout = join_reader(resources.stdout_reader)?;
    let stderr = join_reader(resources.stderr_reader)?;
    if stdout.exceeded_limit || stderr.exceeded_limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bounded command output exceeded capture limit",
        ));
    }
    Ok(Output {
        status,
        stdout: stdout.bytes,
        stderr: stderr.bytes,
    })
}

fn reader_finished(reader: Option<&ReaderHandle>) -> bool {
    reader.is_none_or(JoinHandle::is_finished)
}

fn join_reader(reader: Option<ReaderHandle>) -> io::Result<CapturedStream> {
    match reader {
        Some(reader) => reader
            .join()
            .map_err(|_| io::Error::other("command output reader failed"))?,
        None => Ok(CapturedStream::default()),
    }
}

fn reaper_sender() -> io::Result<Sender<ChildResources>> {
    let reaper = REAPER_SENDER.get_or_init(|| Mutex::new(None));
    let mut sender = reaper
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(sender) = sender.as_ref() {
        return Ok(sender.clone());
    }
    let (new_sender, receiver) = mpsc::channel::<ChildResources>();
    thread::Builder::new()
        .name("velnor-service-reaper".to_owned())
        .spawn(move || {
            while let Ok(resources) = receiver.recv() {
                resources.reap_until_quiescent();
            }
        })?;
    *sender = Some(new_sender.clone());
    Ok(new_sender)
}

fn defer_reap(resources: ChildResources, reaper: &Sender<ChildResources>) {
    if !resources.child_identity_pinned {
        quarantine(resources);
        return;
    }
    if let Err(error) = reaper.send(resources) {
        quarantine(error.0);
    }
}

fn quarantine(resources: ChildResources) {
    QUARANTINED_CHILDREN
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(resources);
}

fn sleep_until(deadline: Instant) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        thread::sleep(remaining.min(POLL_INTERVAL));
    }
}

fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "command deadline elapsed")
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
