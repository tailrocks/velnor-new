//! Bounded process-group and direct-child cleanup for live advisory scans.

use std::io;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use super::wait_until;

pub(crate) fn terminate_process_tree(child: &mut Child, deadline: Instant) -> io::Result<()> {
    let mut command = process_tree_terminator(child.id());
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut terminator = match command.spawn() {
        Ok(terminator) => terminator,
        Err(error) => return Err(cleanup_direct_child(child, deadline, &error)),
    };
    let status = match wait_until(&mut terminator, deadline) {
        Ok(Some(status)) => status,
        Ok(None) => {
            let error = stop_cleanup_command(
                &mut terminator,
                deadline,
                &io::Error::other("process-tree cleanup command timed out"),
            );
            return Err(cleanup_direct_child(child, deadline, &error));
        }
        Err(error) => {
            let error = stop_cleanup_command(&mut terminator, deadline, &error);
            return Err(cleanup_direct_child(child, deadline, &error));
        }
    };
    if status.success() {
        return Ok(());
    }
    Err(cleanup_direct_child(
        child,
        deadline,
        &io::Error::other("process-tree cleanup command failed"),
    ))
}

fn cleanup_direct_child(child: &mut Child, deadline: Instant, source: &io::Error) -> io::Error {
    let mut detail = source.to_string();
    let should_kill = match child.try_wait() {
        Ok(Some(_)) => false,
        Ok(None) => true,
        Err(error) => {
            detail.push_str("; child wait failed before kill: ");
            detail.push_str(&error.to_string());
            true
        }
    };
    if should_kill && let Err(error) = child.kill() {
        detail.push_str("; direct child kill failed: ");
        detail.push_str(&error.to_string());
    }
    match wait_until(child, deadline) {
        Ok(Some(_)) => {}
        Ok(None) => detail.push_str("; direct child cleanup timed out"),
        Err(error) => {
            detail.push_str("; direct child wait failed: ");
            detail.push_str(&error.to_string());
        }
    }
    io::Error::other(detail)
}

fn stop_cleanup_command(command: &mut Child, deadline: Instant, source: &io::Error) -> io::Error {
    let mut detail = source.to_string();
    if let Err(error) = command.kill() {
        detail.push_str("; cleanup command kill failed: ");
        detail.push_str(&error.to_string());
    }
    match wait_until(command, deadline) {
        Ok(Some(_)) => {}
        Ok(None) => detail.push_str("; cleanup command reaping timed out"),
        Err(error) => {
            detail.push_str("; cleanup command wait failed: ");
            detail.push_str(&error.to_string());
        }
    }
    io::Error::other(detail)
}

fn process_tree_terminator(process_id: u32) -> Command {
    #[cfg(unix)]
    {
        let mut command = Command::new("/bin/kill");
        command.args(["-KILL", "--", &format!("-{process_id}")]);
        command
    }
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill.exe");
        command.args(["/PID", &process_id.to_string(), "/T", "/F"]);
        command
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = process_id;
        Command::new("false")
    }
}
