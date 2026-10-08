use std::path::Path;
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use super::{
    BINARY, CONFIG, Manager, ServiceFault, Systemctl, UNIT, ensure_no_pending_unit_job, is_running,
    is_stopped, read_identity_snapshot, read_snapshot, verify_identity_contract,
    verify_load_credential, verify_package_contract, verify_service_environment,
};

pub(super) fn stop_for_config(config_path: &Path, state_path: &Path) -> ExitCode {
    stop_for_config_with_timeout(config_path, state_path, None)
}

pub(super) fn stop_for_disconnect(
    config_path: &Path,
    state_path: &Path,
    timeout_override: Option<u64>,
) -> ExitCode {
    stop_for_config_with_timeout(config_path, state_path, timeout_override)
}

fn stop_for_config_with_timeout(
    config_path: &Path,
    state_path: &Path,
    timeout_override: Option<u64>,
) -> ExitCode {
    let configured_timeout = match super::stopped::validated_drain_timeout(config_path) {
        Ok(timeout) => timeout,
        Err(fault) => return fail(fault),
    };
    let timeout_secs = match timeout_override {
        Some(timeout) if timeout > 0 && timeout <= configured_timeout => timeout,
        Some(_) => return fail(ServiceFault::InvalidConfig),
        None => configured_timeout,
    };
    let Some(deadline) = Instant::now().checked_add(Duration::from_secs(timeout_secs)) else {
        return fail(ServiceFault::InvalidConfig);
    };
    let mut manager = Systemctl::bounded_until(deadline);
    match stop_with(&mut manager, timeout_secs, deadline, |deadline| {
        run_drain_wait(state_path, timeout_secs, deadline)
    }) {
        Ok(()) => {
            println!("service stopped after proof-backed drain");
            ExitCode::SUCCESS
        }
        Err(fault) => fail(fault),
    }
}

fn fail(fault: ServiceFault) -> ExitCode {
    eprintln!("{}", fault.message());
    ExitCode::from(1)
}

pub(super) fn stop_with(
    manager: &mut impl Manager,
    timeout_secs: u64,
    deadline: Instant,
    drain: impl FnOnce(Instant) -> Result<(), ServiceFault>,
) -> Result<(), ServiceFault> {
    ensure_before_deadline(deadline)?;
    verify_stoppable(manager, timeout_secs)?;
    ensure_before_deadline(deadline)?;
    drain(deadline)?;
    ensure_before_deadline(deadline)?;
    verify_stoppable(manager, timeout_secs)?;
    ensure_before_deadline(deadline)?;
    if !super::manager_call(manager, &["stop", UNIT])? {
        return Err(ServiceFault::StopNotVerified);
    }
    ensure_before_deadline(deadline)?;
    super::stopped::verify_stopped(manager, timeout_secs)?;
    ensure_before_deadline(deadline)
}

fn ensure_before_deadline(deadline: Instant) -> Result<(), ServiceFault> {
    if Instant::now() >= deadline {
        Err(ServiceFault::DrainDeadline)
    } else {
        Ok(())
    }
}

fn verify_stoppable(manager: &mut impl Manager, timeout_secs: u64) -> Result<(), ServiceFault> {
    let snapshot = read_snapshot(manager)?;
    verify_package_contract(&snapshot, timeout_secs)?;
    if (!is_running(&snapshot) && !is_stopped(&snapshot)) || snapshot.result != "success" {
        return Err(ServiceFault::UnknownState);
    }
    match snapshot.job.as_deref() {
        Some("") => {}
        Some(_) => return Err(ServiceFault::PendingJob),
        None => return Err(ServiceFault::UnknownState),
    }
    verify_load_credential(manager)?;
    verify_service_environment(manager)?;
    verify_identity_contract(&read_identity_snapshot(manager)?)?;
    ensure_no_pending_unit_job(manager)
}

fn run_drain_wait(
    state_path: &Path,
    timeout_secs: u64,
    deadline: Instant,
) -> Result<(), ServiceFault> {
    let mut command = runuser_command(state_path, timeout_secs);
    let output = super::process::output_until(&mut command, Some(deadline)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::TimedOut {
            ServiceFault::DrainDeadline
        } else {
            ServiceFault::DrainUnavailable
        }
    })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ServiceFault::DrainUnavailable)
    }
}

pub(super) fn runuser_command(state_path: &Path, timeout_secs: u64) -> Command {
    let mut command = Command::new("/usr/sbin/runuser");
    command.args([
        "--user",
        "velnor",
        "--group",
        "velnor",
        "--supp-group",
        "docker",
        "--",
        BINARY,
        "--config",
        CONFIG,
        "--state",
    ]);
    command
        .arg(state_path)
        .args(["drain", "--wait", "--timeout-secs"])
        .arg(timeout_secs.to_string());
    command
}
