use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

use super::{ChildResources, spawn_reader, terminate_without_readers};

pub(super) fn start_output_readers(
    command: &mut Command,
    cleanup_sender: &Sender<ChildResources>,
) -> io::Result<ChildResources> {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command.spawn()?;
    let Some(stdout) = child.stdout.take() else {
        return Err(terminate_without_readers(
            child,
            io::ErrorKind::BrokenPipe,
            cleanup_sender,
        ));
    };
    let Some(stderr) = child.stderr.take() else {
        return Err(terminate_without_readers(
            child,
            io::ErrorKind::BrokenPipe,
            cleanup_sender,
        ));
    };

    let mut resources = ChildResources {
        child,
        child_identity_pinned: true,
        stdout_reader: None,
        stderr_reader: None,
    };
    resources.stdout_reader = match spawn_reader(stdout, "velnor-service-stdout") {
        Ok(handle) => Some(handle),
        Err(error) => {
            drop(stderr);
            resources.start_cleanup(cleanup_sender);
            return Err(error);
        }
    };
    resources.stderr_reader = match spawn_reader(stderr, "velnor-service-stderr") {
        Ok(handle) => Some(handle),
        Err(error) => {
            resources.start_cleanup(cleanup_sender);
            return Err(error);
        }
    };
    Ok(resources)
}
