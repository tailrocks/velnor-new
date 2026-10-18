use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use velnor_runner_host::{HostError, wait_for_drain};

use super::DRAIN_WAIT_BUDGET;
use super::connection_state::admission_lock;
use super::marker::{
    StateWriteError, marker_token, remove_marker_path, report_marker_write_error, write_marker,
};

pub(super) fn remove_flag(state: &Path, name: &str) -> ExitCode {
    let Ok(_lock) = admission_lock(state) else {
        eprintln!("Admission state is locked.");
        return ExitCode::from(1);
    };
    match remove_marker(state, name) {
        Ok(true) => println!("global drain removed"),
        Ok(false) => println!("global drain already clear"),
        Err(StateWriteError::Failed) => {
            eprintln!("Resume failed.");
            return ExitCode::from(1);
        }
        Err(StateWriteError::Uncertain) => {
            eprintln!("Resume state is uncertain.");
            return ExitCode::from(1);
        }
    }
    ExitCode::SUCCESS
}

pub(super) fn remove_marker(state: &Path, name: &str) -> Result<bool, StateWriteError> {
    remove_marker_path(&state.join(name))
}

pub(super) fn drain(state: &Path, wait: bool) -> ExitCode {
    drain_with(state, wait, wait_for_drain)
}

pub(super) fn drain_with<F>(state: &Path, wait: bool, wait_for_drain: F) -> ExitCode
where
    F: FnOnce(&Path, Duration) -> Result<bool, HostError>,
{
    let token = {
        let Ok(_lock) = admission_lock(state) else {
            eprintln!("Admission state is locked.");
            return ExitCode::from(1);
        };
        match write_marker(&state.join("drain")) {
            Ok(token) => token,
            Err(error) => {
                report_marker_write_error(error);
                return ExitCode::from(1);
            }
        }
    };
    if !wait {
        println!("draining");
        return ExitCode::SUCCESS;
    }
    match wait_for_drain(state, DRAIN_WAIT_BUDGET) {
        Ok(true) => {
            let Ok(_lock) = admission_lock(state) else {
                eprintln!("Admission state is locked.");
                return ExitCode::from(1);
            };
            if marker_token(&state.join("drain")).ok().flatten() != Some(token) {
                eprintln!("Drain ended before work finished.");
                return ExitCode::from(1);
            }
            println!("drained");
            ExitCode::SUCCESS
        }
        Ok(false) => {
            let Ok(_lock) = admission_lock(state) else {
                eprintln!("Admission state is locked.");
                return ExitCode::from(1);
            };
            match marker_token(&state.join("drain")) {
                Ok(Some(current)) if current == token => {
                    eprintln!("Drain timed out. The drain stays active.");
                }
                Ok(_) => eprintln!("Drain ended before work finished."),
                Err(_) => eprintln!("Drain state is uncertain."),
            }
            ExitCode::from(1)
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}
