use std::fs;
use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::thread;
use std::time::{Duration, Instant};

use rustix::fs::{Mode, OFlags, open};

use crate::error::HostError;

use super::write_projection;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn private_scope() -> Result<(std::path::PathBuf, OwnedFd), String> {
    let id = NEXT_DIRECTORY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "velnor-projection-publish-{}-{id}",
        std::process::id()
    ));
    fs::create_dir(&path).map_err(|error| error.to_string())?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let directory = open(
        &path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| format!("{error:?}"))?;
    Ok((path, directory))
}

fn write(scope: &OwnedFd, name: &str, bytes: &[u8]) -> Result<(), HostError> {
    let started = Instant::now();
    write_projection(
        scope,
        name,
        bytes,
        started,
        started + Duration::from_secs(10),
        &Default::default(),
    )
}

#[test]
fn stale_partial_stage_does_not_block_exact_retry() -> Result<(), String> {
    let (path, scope) = private_scope()?;
    let final_name = "i17-g4-runner-logs.json";
    let stale = path.join(format!(".{final_name}.tmp-crashed-writer"));
    fs::write(&stale, b"partial bytes before crash").map_err(|error| error.to_string())?;
    fs::set_permissions(&stale, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;

    let expected = br#"{"schema":1,"phase":"runner_trace"}"#;
    write(&scope, final_name, expected).map_err(|error| format!("{error:?}"))?;
    if fs::read(path.join(final_name)).map_err(|error| error.to_string())? != expected {
        return Err("The retry did not publish the complete record.".to_owned());
    }
    if fs::read(&stale).map_err(|error| error.to_string())? != b"partial bytes before crash" {
        return Err("The retry changed the existing temporary file.".to_owned());
    }
    fs::remove_dir_all(path).map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn mismatched_final_is_never_replaced() -> Result<(), String> {
    let (path, scope) = private_scope()?;
    let final_name = "i18-g4-runner-logs.json";
    let final_path = path.join(final_name);
    fs::write(&final_path, b"foreign final projection").map_err(|error| error.to_string())?;
    fs::set_permissions(&final_path, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;

    if write(&scope, final_name, b"replacement") != Err(HostError::Ownership) {
        return Err("The writer accepted a final file with different content.".to_owned());
    }
    if fs::read(&final_path).map_err(|error| error.to_string())? != b"foreign final projection" {
        return Err("The writer changed the final file with different content.".to_owned());
    }
    fs::remove_dir_all(path).map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn concurrent_exact_writes_publish_one_complete_final() -> Result<(), String> {
    let (path, scope) = private_scope()?;
    let scope = Arc::new(scope);
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let name = "i19-g4-runner-logs.json";
    let bytes = Arc::new(br#"{"schema":1,"phase":"runner_trace","events":[]}"#.to_vec());
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let scope = Arc::clone(&scope);
            let barrier = Arc::clone(&barrier);
            let bytes = Arc::clone(&bytes);
            thread::spawn(move || {
                barrier.wait();
                write(&scope, name, &bytes)
            })
        })
        .collect();
    for worker in workers {
        worker
            .join()
            .map_err(|_| "The record writer stopped unexpectedly.".to_owned())?
            .map_err(|error| format!("The writers cannot publish the record. Error: {error:?}."))?;
    }
    if fs::read(path.join(name)).map_err(|error| error.to_string())? != *bytes {
        return Err("The writers did not publish the expected record.".to_owned());
    }
    let entries = fs::read_dir(&path)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if entries.len() != 1 || entries[0].file_name() != name {
        return Err("The directory contains unexpected files after publication.".to_owned());
    }
    drop(scope);
    fs::remove_dir_all(path).map_err(|error| error.to_string())?;
    Ok(())
}
