use super::*;
use std::os::unix::process::CommandExt;
use std::process::ExitStatus;
use std::thread;
use std::time::{Duration, Instant};

pub(in crate::stock_mbx_bundle_cli_tests) struct GatedRun {
    pub(in crate::stock_mbx_bundle_cli_tests) step: StepResult,
    pub(in crate::stock_mbx_bundle_cli_tests) hook: String,
    pub(in crate::stock_mbx_bundle_cli_tests) exit_code: Option<i32>,
    pub(in crate::stock_mbx_bundle_cli_tests) timed_out: bool,
}

#[derive(Clone, Copy)]
pub(in crate::stock_mbx_bundle_cli_tests) enum GateAction<'a> {
    Cancel,
    Release(&'a Path),
}

pub(in crate::stock_mbx_bundle_cli_tests) struct GatedShell<'a> {
    pub(in crate::stock_mbx_bundle_cli_tests) step: &'a Step,
    pub(in crate::stock_mbx_bundle_cli_tests) stage: &'a Stage,
    pub(in crate::stock_mbx_bundle_cli_tests) outputs: &'a BTreeMap<String, String>,
    pub(in crate::stock_mbx_bundle_cli_tests) cwd: &'a Path,
    pub(in crate::stock_mbx_bundle_cli_tests) temp: &'a Path,
    pub(in crate::stock_mbx_bundle_cli_tests) extra: &'a [(&'a str, &'a OsStr)],
    pub(in crate::stock_mbx_bundle_cli_tests) marker: &'a Path,
    pub(in crate::stock_mbx_bundle_cli_tests) label: &'a str,
    pub(in crate::stock_mbx_bundle_cli_tests) action: GateAction<'a>,
}

pub(in crate::stock_mbx_bundle_cli_tests) fn run_shell_gated(
    request: &GatedShell<'_>,
) -> Result<GatedRun, String> {
    let mut invocation = super::invocation(
        request.step,
        request.stage,
        request.outputs,
        request.cwd,
        request.temp,
        request.extra,
        request.label,
    )?;
    let stdout_path = request.stage.root.join(format!("{}.stdout", request.label));
    let stderr_path = request.stage.root.join(format!("{}.stderr", request.label));
    let stdout = output_file(&stdout_path)?;
    let stderr = output_file(&stderr_path)?;
    invocation
        .command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    invocation.command.process_group(0);
    let mut child = invocation
        .command
        .spawn()
        .map_err(|err| format!("spawn {}: {err}", request.step.name))?;
    let hook = match wait_for_marker(
        request.marker,
        &mut child,
        &stdout_path,
        &stderr_path,
        Duration::from_secs(30),
    ) {
        Ok(value) => value,
        Err(err) => {
            if child.try_wait().map_err(|wait| wait.to_string())?.is_none() {
                terminate_owned_shell_tree(child.id(), None)?;
                child.wait().map_err(|wait| wait.to_string())?;
            }
            return Err(err);
        }
    };
    match request.action {
        GateAction::Cancel => signal_group("-TERM", &child.id().to_string())?,
        GateAction::Release(gate) => {
            fs::write(gate, b"release\n").map_err(|err| err.to_string())?;
        }
    }
    let (status, timed_out) = wait_for_exit(&mut child, &hook, Duration::from_secs(15))?;
    let result = Output {
        status,
        stdout: fs::read(&stdout_path).map_err(|err| err.to_string())?,
        stderr: fs::read(&stderr_path).map_err(|err| err.to_string())?,
    };
    let exit_code = result.status.code();
    let step = super::finish(request.step, request.stage, &invocation, &result)?;
    Ok(GatedRun {
        step,
        hook,
        exit_code,
        timed_out,
    })
}

fn wait_for_marker(
    marker: &Path,
    child: &mut std::process::Child,
    stdout_path: &Path,
    stderr_path: &Path,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match fs::symlink_metadata(marker) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                return fs::read_to_string(marker).map_err(|err| err.to_string());
            }
            Ok(_) => return Err("metadata hook marker is not a regular file".to_owned()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.to_string()),
        }
        if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
            let stdout = fs::read(stdout_path).map_err(|err| err.to_string())?;
            let stderr = fs::read(stderr_path).map_err(|err| err.to_string())?;
            return Err(format!(
                "generated shell exited before metadata barrier: status={status}, stdout={}, stderr={}",
                String::from_utf8_lossy(&stdout).trim(),
                String::from_utf8_lossy(&stderr).trim()
            ));
        }
        if Instant::now() >= deadline {
            return Err("generated shell did not reach the CARGO barrier".to_owned());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn wait_for_exit(
    child: &mut std::process::Child,
    hook: &str,
    timeout: Duration,
) -> Result<(ExitStatus, bool), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
            return Ok((status, false));
        }
        if Instant::now() >= deadline {
            terminate_owned_shell_tree(child.id(), Some(hook))?;
            return child
                .wait()
                .map(|status| (status, true))
                .map_err(|err| err.to_string());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn terminate_owned_shell_tree(shell_pid: u32, hook: Option<&str>) -> Result<(), String> {
    if let Some(group) = hook.and_then(|text| hook_value(text, "pgid")) {
        signal_group("-KILL", group)?;
    }
    for group in child_process_groups(shell_pid)? {
        signal_group("-KILL", &group)?;
    }
    signal_pid("-KILL", shell_pid)
}

fn child_process_groups(shell_pid: u32) -> Result<Vec<String>, String> {
    let shell_group = shell_pid.to_string();
    let listing = Command::new("/usr/bin/ps")
        .args(["-o", "pid=,pgid=", "--ppid", shell_group.as_str()])
        .output()
        .map_err(|err| err.to_string())?;
    if !listing.status.success() {
        return Err(format!(
            "cannot inspect generated shell children: status={}, stderr={}",
            listing.status,
            String::from_utf8_lossy(&listing.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&listing.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1).map(str::to_owned))
        .filter(|group| group != &shell_group)
        .collect())
}

fn hook_value<'a>(hook: &'a str, name: &str) -> Option<&'a str> {
    hook.lines()
        .find_map(|line| line.strip_prefix(&format!("{name}=")))
}

fn signal_pid(signal: &str, pid: u32) -> Result<(), String> {
    signal_target(signal, &pid.to_string())
}

fn signal_group(signal: &str, group: &str) -> Result<(), String> {
    signal_target(signal, &format!("-{group}"))
}

fn signal_target(signal: &str, target: &str) -> Result<(), String> {
    let mut command = Command::new("/bin/kill");
    command.arg(signal);
    if target.starts_with('-') {
        command.arg("--");
    }
    let status = command
        .arg(target)
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() || signal == "-KILL" {
        Ok(())
    } else {
        Err(format!("kill {signal} {target} returned {status}"))
    }
}

fn output_file(path: &Path) -> Result<std::fs::File, String> {
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(|err| err.to_string())
}
