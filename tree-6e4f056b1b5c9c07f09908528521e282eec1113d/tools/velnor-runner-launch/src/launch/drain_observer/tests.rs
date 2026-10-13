use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::runtime::Builder;
use velnor_runner_host::worker::{
    OwnedDockerResource, OwnedDockerResourceKind, ProtectedStateDirectory,
    validate_protected_state_directory,
};
use velnor_runner_journal::journal::Journal;

use super::{request_drain_protected_blocking, wait_drained_protected_blocking_with};
use crate::launch::control::{DrainOutcome, DrainRequestOutcome, DrainUnknown};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch {
    path: PathBuf,
    displaced: Option<PathBuf>,
}

impl Scratch {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "velnor-drain-observer-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).map_err(|error| error.to_string())?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            path,
            displaced: None,
        })
    }

    fn token(&self) -> Result<ProtectedStateDirectory, String> {
        validate_protected_state_directory(&self.path).map_err(|error| error.to_string())
    }

    fn journal_path(&self) -> PathBuf {
        self.path.join("launch.db")
    }

    fn initialize(&self, drain: bool) -> Result<ProtectedStateDirectory, String> {
        let protected = self.token()?;
        let identity = protected.identity().map_err(|error| error.to_string())?;
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        runtime
            .block_on(async {
                let journal = Journal::open_protected_at(
                    &self.journal_path(),
                    identity.device(),
                    identity.inode(),
                )
                .await?;
                if drain {
                    journal.request_drain().await?;
                }
                Ok::<(), velnor_runner_host::HostError>(())
            })
            .map_err(|error| error.to_string())?;
        Ok(protected)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.path));
        if let Some(displaced) = &self.displaced {
            drop(std::fs::remove_dir_all(displaced));
        }
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}

#[test]
fn protected_request_is_observed_through_the_same_state_capability() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let protected = scratch.initialize(false)?;
    let request = request_drain_protected_blocking(&protected, &scratch.journal_path(), deadline())
        .map_err(|error| format!("unexpected control failure: {error}"))?;
    assert_eq!(request, DrainRequestOutcome::Requested);

    let inventory_calls = AtomicU64::new(0);
    let result = wait_drained_protected_blocking_with(
        &protected,
        &scratch.journal_path(),
        "unix:///unused.sock",
        deadline(),
        |_, _| {
            inventory_calls.fetch_add(1, Ordering::Relaxed);
            async { Err(()) }
        },
    );
    assert_eq!(
        result,
        DrainOutcome::Unknown(DrainUnknown::OwnershipInventoryUnavailable)
    );
    assert_eq!(inventory_calls.load(Ordering::Relaxed), 1);
    Ok(())
}

#[test]
fn observer_refuses_an_unfenced_journal_before_inventory() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let protected = scratch.initialize(false)?;
    let inventory_calls = AtomicU64::new(0);
    let result = wait_drained_protected_blocking_with(
        &protected,
        &scratch.journal_path(),
        "unix:///unused.sock",
        deadline(),
        |_, _| {
            inventory_calls.fetch_add(1, Ordering::Relaxed);
            async { Ok(Vec::new()) }
        },
    );
    assert_eq!(
        result,
        DrainOutcome::Unknown(DrainUnknown::AdmissionNotFenced)
    );
    assert_eq!(inventory_calls.load(Ordering::Relaxed), 0);
    Ok(())
}

#[test]
fn observer_never_reports_drained_when_inventory_is_incomplete() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let protected = scratch.initialize(true)?;
    let result = wait_drained_protected_blocking_with(
        &protected,
        &scratch.journal_path(),
        "unix:///unused.sock",
        deadline(),
        |_, _| async { Err(()) },
    );
    assert_eq!(
        result,
        DrainOutcome::Unknown(DrainUnknown::OwnershipInventoryUnavailable)
    );
    Ok(())
}

#[test]
fn final_snapshot_rejects_a_fence_cleared_during_inventory() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let protected = scratch.initialize(true)?;
    let identity = protected.identity().map_err(|error| error.to_string())?;
    let journal_path = scratch.journal_path();
    let inventory_journal_path = journal_path.clone();
    let result = wait_drained_protected_blocking_with(
        &protected,
        &journal_path,
        "unix:///unused.sock",
        deadline(),
        move |_, _| {
            let journal_path = inventory_journal_path.clone();
            async move {
                let journal = Journal::open_existing_protected_at(
                    &journal_path,
                    identity.device(),
                    identity.inode(),
                )
                .await
                .map_err(|_| ())?;
                journal.resume().await.map_err(|_| ())?;
                Ok(Vec::new())
            }
        },
    );
    assert_eq!(
        result,
        DrainOutcome::Unknown(DrainUnknown::AdmissionNotFenced)
    );
    Ok(())
}

#[test]
fn observer_reports_drained_only_after_a_final_empty_snapshot() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let protected = scratch.initialize(true)?;
    let result = wait_drained_protected_blocking_with(
        &protected,
        &scratch.journal_path(),
        "unix:///unused.sock",
        deadline(),
        |_, _| async { Ok(Vec::new()) },
    );
    assert_eq!(result, DrainOutcome::Drained);
    Ok(())
}

#[test]
fn observer_waits_until_deadline_when_owned_resources_remain() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let protected = scratch.initialize(true)?;
    let cutoff = Instant::now() + Duration::from_millis(40);
    let result = wait_drained_protected_blocking_with(
        &protected,
        &scratch.journal_path(),
        "unix:///unused.sock",
        cutoff,
        |_, _| async {
            Ok(vec![OwnedDockerResource {
                kind: OwnedDockerResourceKind::Container,
                id_or_name: "resource-id".to_owned(),
                names: vec!["/velnor-worker".to_owned()],
                worker: "worker".to_owned(),
                role: "runner".to_owned(),
                labels: BTreeMap::new(),
            }])
        },
    );
    assert_eq!(result, DrainOutcome::Deadline);
    Ok(())
}

#[test]
fn state_path_replacement_is_unknown_and_does_not_query_inventory() -> Result<(), String> {
    let mut scratch = Scratch::new()?;
    let protected = scratch.initialize(false)?;
    let displaced = scratch.path.with_extension("displaced");
    std::fs::rename(&scratch.path, &displaced).map_err(|error| error.to_string())?;
    std::fs::create_dir(&scratch.path).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&scratch.path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    scratch.displaced = Some(displaced);

    assert!(matches!(
        request_drain_protected_blocking(&protected, &scratch.journal_path(), deadline()),
        Err(crate::launch::control::ControlOpenError::JournalUnavailable)
    ));
    assert!(!scratch.journal_path().exists());

    let inventory_calls = AtomicU64::new(0);
    let result = wait_drained_protected_blocking_with(
        &protected,
        &scratch.journal_path(),
        "unix:///unused.sock",
        deadline(),
        |_, _| {
            inventory_calls.fetch_add(1, Ordering::Relaxed);
            async { Ok(Vec::new()) }
        },
    );
    assert_eq!(
        result,
        DrainOutcome::Unknown(DrainUnknown::JournalUnavailable)
    );
    assert_eq!(inventory_calls.load(Ordering::Relaxed), 0);
    Ok(())
}
