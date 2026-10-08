#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use super::super::{LinuxDaemonError, open_runtime_state};
use velnor_runner_journal::journal::Journal;

#[cfg(unix)]
#[tokio::test]
async fn journal_open_is_pinned_to_the_retained_prelock_directory_identity() -> Result<(), String> {
    let scratch = crate::launch::harness::Scratch::new("linux-retained-state-directory")
        .map_err(|error| error.to_string())?;
    let state = scratch
        .file()
        .parent()
        .ok_or_else(|| "scratch journal has no parent".to_owned())?
        .to_path_buf();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let protected = velnor_runner_host::worker::validate_protected_state_directory(&state)
        .map_err(|error| error.to_string())?;
    let identity = protected.identity().map_err(|error| error.to_string())?;
    let path = state.join("launch.db");

    assert!(
        Journal::open_protected_at(&path, identity.device().wrapping_add(1), identity.inode(),)
            .await
            .is_err()
    );
    assert!(
        !path.exists(),
        "identity mismatch must fail before DB creation"
    );

    let journal = Journal::open_protected_at(&path, identity.device(), identity.inode())
        .await
        .map_err(|error| error.to_string())?;
    journal.rows().await.map_err(|error| error.to_string())?;

    let displaced = state.with_extension("retained");
    std::fs::rename(&state, &displaced).map_err(|error| error.to_string())?;
    std::fs::create_dir(&state).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    assert!(journal.rows().await.is_err());
    assert!(
        !state.join("launch.db").exists(),
        "replacement path must not receive journal operations"
    );
    std::fs::remove_dir(&state).map_err(|error| error.to_string())?;
    std::fs::rename(displaced, &state).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn untrusted_state_alias_is_rejected_before_journal_creation() -> Result<(), String> {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let scratch = crate::launch::harness::Scratch::new("linux-unsafe-state")
        .map_err(|error| error.to_string())?;
    let scratch_root = scratch
        .file()
        .parent()
        .ok_or_else(|| "scratch journal has no parent directory".to_owned())?
        .to_path_buf();
    let state = scratch_root.join("state");
    std::fs::create_dir(&state).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let alias = scratch_root.join("state-alias");
    symlink(&state, &alias).map_err(|error| error.to_string())?;

    let protected = velnor_runner_host::worker::validate_protected_state_directory(&state)
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        open_runtime_state(&alias, &protected).await,
        Err(LinuxDaemonError::JournalUnavailable)
    ));
    assert!(!state.join("launch.db").exists());
    assert!(!state.join("diagnostics").exists());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_journal_leaf_is_rejected_without_bootstrap_or_target_mutation()
-> Result<(), String> {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let scratch = crate::launch::harness::Scratch::new("linux-unsafe-journal")
        .map_err(|error| error.to_string())?;
    let scratch_root = scratch
        .file()
        .parent()
        .ok_or_else(|| "scratch journal has no parent directory".to_owned())?
        .to_path_buf();
    let state = scratch_root.join("state");
    std::fs::create_dir(&state).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let target = scratch_root.join("target.db");
    std::fs::write(&target, b"leave this database target alone")
        .map_err(|error| error.to_string())?;
    symlink(&target, state.join("launch.db")).map_err(|error| error.to_string())?;

    let protected = velnor_runner_host::worker::validate_protected_state_directory(&state)
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        open_runtime_state(&state, &protected).await,
        Err(LinuxDaemonError::JournalUnavailable)
    ));
    assert_eq!(
        std::fs::read(&target).map_err(|error| error.to_string())?,
        b"leave this database target alone"
    );
    assert!(!state.join("diagnostics").exists());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn journal_and_diagnostics_follow_the_prelock_directory_token_after_path_replacement()
-> Result<(), String> {
    let scratch = crate::launch::harness::Scratch::new("linux-prelock-path-replacement")
        .map_err(|error| error.to_string())?;
    let state = scratch
        .file()
        .parent()
        .ok_or_else(|| "scratch journal has no parent directory".to_owned())?
        .to_path_buf();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let protected = velnor_runner_host::worker::validate_protected_state_directory(&state)
        .map_err(|error| error.to_string())?;
    let (journal, _diagnostics) = open_runtime_state(&state, &protected)
        .await
        .map_err(|error| format!("initial protected open failed: {error:?}"))?;
    journal.rows().await.map_err(|error| error.to_string())?;

    let displaced = state.with_extension("prelock-retained");
    std::fs::rename(&state, &displaced).map_err(|error| error.to_string())?;
    std::fs::create_dir(&state).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;

    assert!(journal.rows().await.is_err());
    assert!(
        !state.join("launch.db").exists(),
        "replacement state path must not receive a Journal open"
    );
    assert!(
        !state.join("diagnostics").exists(),
        "diagnostics remain relative to the retained state descriptor"
    );

    std::fs::remove_dir(&state).map_err(|error| error.to_string())?;
    std::fs::rename(displaced, &state).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn replacement_after_token_retention_is_rejected_before_first_journal_bootstrap()
-> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let scratch = crate::launch::harness::Scratch::new("linux-preopen-path-replacement")
        .map_err(|error| error.to_string())?;
    let state = scratch
        .file()
        .parent()
        .ok_or_else(|| "scratch journal has no parent directory".to_owned())?
        .to_path_buf();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let protected = velnor_runner_host::worker::validate_protected_state_directory(&state)
        .map_err(|error| error.to_string())?;
    let _diagnostics_on_retained_directory = protected
        .open_diagnostics_store()
        .map_err(|error| error.to_string())?;

    let displaced = state.with_extension("preopen-retained");
    std::fs::rename(&state, &displaced).map_err(|error| error.to_string())?;
    std::fs::create_dir(&state).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;

    assert!(matches!(
        open_runtime_state(&state, &protected).await,
        Err(LinuxDaemonError::JournalUnavailable)
    ));
    assert!(
        !state.join("launch.db").exists(),
        "replaced path must not receive a journal bootstrap"
    );
    assert!(
        !state.join("diagnostics").exists(),
        "diagnostics must not be created below the replacement path"
    );
    let _diagnostics_after_replacement = protected
        .open_diagnostics_store()
        .map_err(|error| error.to_string())?;
    assert!(displaced.join("diagnostics").is_dir());
    assert!(!state.join("diagnostics").exists());

    std::fs::remove_dir(&state).map_err(|error| error.to_string())?;
    std::fs::rename(displaced, &state).map_err(|error| error.to_string())?;
    Ok(())
}
