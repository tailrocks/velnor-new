use std::error::Error;
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::config::HostConfig;
use crate::config_test_support::{basic_connection, host_config};
use crate::daemon_lock::DaemonLock;
use crate::error::HostError;
use crate::host_controller::run_host_blocking;
use crate::launch_blocking::ListenFault;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> io::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("velnor-controller-run-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn test_config() -> Result<HostConfig, HostError> {
    let connection = basic_connection(
        "controller-run",
        101,
        "tailrocks/controller-run",
        "controller-run",
        "controller-run-scale-set",
    );
    HostConfig::parse(&host_config(&[connection], 1))
}

#[test]
fn duplicate_run_is_rejected_before_state_access() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let daemon_lock = DaemonLock::try_acquire(&scratch.0.join("daemon.lock"))?;
    let active_run = daemon_lock.begin_controller_run()?;
    let missing_state = scratch.0.join("missing-state");
    let config = test_config()?;

    let duplicate = run_host_blocking(&config, &missing_state, &daemon_lock);
    assert!(matches!(duplicate, Err(ListenFault::Host(HostError::Lock))));
    assert!(!missing_state.exists());
    drop(active_run);

    let later = run_host_blocking(&config, &missing_state, &daemon_lock);
    assert!(matches!(later, Err(ListenFault::Host(HostError::Path))));
    Ok(())
}

#[test]
fn failed_host_initialization_releases_same_lock_claim() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let daemon_lock = DaemonLock::try_acquire(&scratch.0.join("daemon.lock"))?;
    let missing_state = scratch.0.join("missing-state");
    let config = test_config()?;

    for _ in 0..2 {
        let failed = run_host_blocking(&config, &missing_state, &daemon_lock);
        assert!(matches!(failed, Err(ListenFault::Host(HostError::Path))));
    }
    Ok(())
}
