use std::error::Error;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use crate::gc1_scene::fail;
use crate::gc1_seal_units::group_is_live;

pub(crate) struct StatusFifoBarrier {
    writer: Arc<Mutex<GroupChild>>,
    cleanup: mpsc::Sender<()>,
    group: u32,
    opened: PathBuf,
    started: PathBuf,
    release: PathBuf,
    open_gate: PathBuf,
}

struct GroupChild {
    child: Option<Child>,
    group: u32,
    exit_status: Option<ExitStatus>,
}

struct SpawnedGroupChild {
    writer: Arc<Mutex<GroupChild>>,
    cleanup: mpsc::Sender<()>,
    group: u32,
}

impl GroupChild {
    fn empty() -> Self {
        Self {
            child: None,
            group: 0,
            exit_status: None,
        }
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, Box<dyn Error>> {
        let Some(child) = self.child.as_mut() else {
            return Ok(self.exit_status);
        };
        match child.try_wait()? {
            Some(status) => {
                self.exit_status = Some(status);
                self.child = None;
                Ok(Some(status))
            }
            None => Ok(None),
        }
    }

    fn terminate_and_reap(&mut self) -> Result<(), Box<dyn Error>> {
        let mut observation_error = self.try_wait().err();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut signal_succeeded = false;
        loop {
            if let Err(error) = self.try_wait()
                && observation_error.is_none()
            {
                observation_error = Some(error);
            }
            let group_live = group_is_live(self.group);
            if self.child.is_none() && !group_live {
                return observation_error.map_or(Ok(()), Err);
            }
            if Instant::now() >= deadline {
                return Err(observation_error.unwrap_or_else(|| {
                    fail("owned FIFO child or process group exceeded its reap bound")
                }));
            }
            if group_live && !signal_succeeded {
                match Command::new("kill")
                    .args(["-KILL", "--"])
                    .arg(format!("-{}", self.group))
                    .status()
                {
                    Ok(status) if status.success() => signal_succeeded = true,
                    Ok(_) => {
                        if observation_error.is_none() {
                            observation_error =
                                Some(fail("could not kill the owned FIFO process group"));
                        }
                    }
                    Err(error) => {
                        if observation_error.is_none() {
                            observation_error = Some(error.into());
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_success(&mut self) -> Result<(), Box<dyn Error>> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.try_wait()? {
                if !status.success() {
                    self.terminate_and_reap()?;
                    return Err(fail("owned FIFO process failed"));
                }
                if self.child.is_some() || group_is_live(self.group) {
                    if Instant::now() >= deadline {
                        return Err(fail("owned FIFO process group exceeded its wait bound"));
                    }
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                self.terminate_and_reap()?;
                return Err(fail("owned FIFO process exceeded its wait bound"));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn cleanup_complete(&self) -> bool {
        self.child.is_none() && !group_is_live(self.group)
    }
}

fn lock_writer(
    writer: &Arc<Mutex<GroupChild>>,
) -> Result<MutexGuard<'_, GroupChild>, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match writer.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(_)) => {
                return Err(fail("FIFO child custody lock was poisoned"));
            }
            Err(TryLockError::WouldBlock) if Instant::now() >= deadline => {
                return Err(fail("FIFO child custody lock exceeded its wait bound"));
            }
            Err(TryLockError::WouldBlock) => thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn cleanup_until_complete(writer: &Arc<Mutex<GroupChild>>) {
    loop {
        match writer.try_lock() {
            Ok(mut child) => {
                if child.cleanup_complete() {
                    return;
                }
                drop(child.terminate_and_reap());
                if child.cleanup_complete() {
                    return;
                }
            }
            Err(TryLockError::Poisoned(poisoned)) => {
                let mut child = poisoned.into_inner();
                if child.cleanup_complete() {
                    return;
                }
                drop(child.terminate_and_reap());
                if child.cleanup_complete() {
                    return;
                }
            }
            Err(TryLockError::WouldBlock) => {}
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn spawn_group_child(command: &mut Command) -> Result<SpawnedGroupChild, Box<dyn Error>> {
    let writer = Arc::new(Mutex::new(GroupChild::empty()));
    let mut custody = match writer.try_lock() {
        Ok(custody) => custody,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => {
            return Err(fail("new FIFO child custody lock was already held"));
        }
    };
    let (cleanup_sender, cleanup_receiver) = mpsc::channel();
    let worker_writer = Arc::clone(&writer);
    let worker = thread::Builder::new().spawn(move || {
        if cleanup_receiver.recv().is_ok() {
            cleanup_until_complete(&worker_writer);
        }
    })?;
    let child = command.process_group(0).spawn()?;
    let group = child.id();
    custody.child = Some(child);
    custody.group = group;
    drop(custody);
    drop(worker);
    Ok(SpawnedGroupChild {
        writer,
        cleanup: cleanup_sender,
        group,
    })
}

fn request_cleanup(cleanup: &mpsc::Sender<()>) {
    let _send_result = cleanup.send(());
}

impl StatusFifoBarrier {
    pub(crate) fn started(&self) -> &Path {
        &self.started
    }

    pub(crate) fn opened(&self) -> &Path {
        &self.opened
    }

    pub(crate) fn allow_open(&self) -> Result<(), Box<dyn Error>> {
        fs::write(&self.open_gate, "open\n")?;
        Ok(())
    }

    pub(crate) fn release_and_wait(&mut self) -> Result<(), Box<dyn Error>> {
        fs::write(&self.release, "release\n")?;
        lock_writer(&self.writer)?.wait_success()
    }

    pub(crate) fn process_group(&self) -> u32 {
        self.group
    }

    pub(crate) fn external_cleanup(&self) -> GroupChildGuard {
        GroupChildGuard {
            writer: Arc::clone(&self.writer),
            cleanup: self.cleanup.clone(),
        }
    }
}

impl Drop for StatusFifoBarrier {
    fn drop(&mut self) {
        if let Ok(mut writer) = lock_writer(&self.writer) {
            drop(writer.terminate_and_reap());
        }
        request_cleanup(&self.cleanup);
    }
}

pub(crate) fn start_status_fifo_writer(
    status_path: &Path,
    gh_dir: &Path,
) -> Result<StatusFifoBarrier, Box<dyn Error>> {
    match fs::remove_file(status_path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    if !Command::new("mkfifo").arg(status_path).status()?.success() {
        return Err(fail("could not create the status-read barrier"));
    }
    let name = status_path
        .file_name()
        .ok_or_else(|| fail("invalid status FIFO name"))?
        .to_string_lossy();
    let opened = gh_dir.join(format!("{name}.opened"));
    let started = gh_dir.join(format!("{name}.started"));
    let release = gh_dir.join(format!("{name}.release"));
    let open_gate = gh_dir.join(format!("{name}.open-gate"));
    for marker in [&opened, &started, &release, &open_gate] {
        match fs::remove_file(marker) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.into());
            }
            _ => {}
        }
    }
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg("printf 'started\\n' > \"$4\"; while [ ! -e \"$5\" ]; do sleep 0.01; done; exec 3>\"$1\"; printf 'open\\n' > \"$2\"; while [ ! -e \"$3\" ]; do sleep 0.01; done; printf '0\\n' >&3; exec 3>&-")
        .arg("status-fifo-writer")
        .arg(status_path)
        .arg(&opened)
        .arg(&release)
        .arg(&started)
        .arg(&open_gate);
    let custody = spawn_group_child(&mut command)?;
    Ok(StatusFifoBarrier {
        writer: custody.writer,
        cleanup: custody.cleanup,
        group: custody.group,
        opened,
        started,
        release,
        open_gate,
    })
}

pub(crate) struct GroupChildGuard {
    writer: Arc<Mutex<GroupChild>>,
    cleanup: mpsc::Sender<()>,
}

impl GroupChildGuard {
    pub(crate) fn wait_success(&mut self) -> Result<(), Box<dyn Error>> {
        lock_writer(&self.writer)?.wait_success()
    }

    pub(crate) fn terminate_and_reap(&mut self) -> Result<(), Box<dyn Error>> {
        lock_writer(&self.writer)?.terminate_and_reap()
    }
}

impl Drop for GroupChildGuard {
    fn drop(&mut self) {
        if let Ok(mut child) = lock_writer(&self.writer) {
            drop(child.terminate_and_reap());
        }
        request_cleanup(&self.cleanup);
    }
}

pub(crate) fn spawn_fifo_reader(
    status_path: &Path,
    output_path: &Path,
) -> Result<GroupChildGuard, Box<dyn Error>> {
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg("cat \"$1\" > \"$2\"")
        .arg("status-fifo-reader")
        .arg(status_path)
        .arg(output_path);
    let custody = spawn_group_child(&mut command)?;
    Ok(GroupChildGuard {
        writer: custody.writer,
        cleanup: custody.cleanup,
    })
}

pub(crate) fn terminate_group_bounded(group: u32) -> Result<(), Box<dyn Error>> {
    if group_is_live(group) {
        let status = Command::new("kill")
            .args(["-KILL", "--"])
            .arg(format!("-{group}"))
            .status()?;
        if !status.success() && group_is_live(group) {
            return Err(fail(
                "independent FIFO cleanup could not kill a process group",
            ));
        }
    }
    super::super::wait_until(|| !group_is_live(group), Duration::from_secs(5))
}
