use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);
const MAX_TEMP_DIRECTORY_ATTEMPTS: usize = 1_024;

struct TestTempDir(PathBuf);

impl Drop for TestTempDir {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0)
            && error.kind() != io::ErrorKind::NotFound
        {
            eprintln!("velnor_temp_test_cleanup_failed:{:?}", error.kind());
        }
    }
}

/// Reserve a temporary directory with process-wide unique allocation.
pub(crate) fn unique_temp_dir(prefix: &str) -> io::Result<PathBuf> {
    unique_temp_dir_in(&std::env::temp_dir(), prefix)
}

/// Reserve a temporary directory below `parent`, retrying only occupied names.
pub(crate) fn unique_temp_dir_in(parent: &Path, prefix: &str) -> io::Result<PathBuf> {
    unique_temp_dir_in_with_counter(parent, prefix, &NEXT_TEMP_DIRECTORY_ID)
}

fn unique_temp_dir_in_with_counter(
    parent: &Path,
    prefix: &str,
    counter: &AtomicU64,
) -> io::Result<PathBuf> {
    let process_id = std::process::id();
    for _ in 0..MAX_TEMP_DIRECTORY_ATTEMPTS {
        let id = counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| io::Error::other("temporary directory ID space exhausted"))?;
        let candidate = parent.join(format!("{prefix}-{process_id}-{id}"));
        match std::fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "temporary directory namespace exhausted after 1024 occupied names",
    ))
}

#[test]
fn temp_directory_allocation_handles_collisions_parallel_calls_and_restarts() -> io::Result<()> {
    const PARALLEL_CALLS: usize = 12;
    let parent = unique_temp_dir("velnor-temp-allocation-test")?;
    let _cleanup = TestTempDir(parent.clone());
    let process_id = std::process::id();
    let prefix = String::from("velnor-candidate");
    let mut created = Vec::with_capacity(PARALLEL_CALLS + 3);
    let stale = parent.join(format!("{prefix}-{process_id}-0"));
    std::fs::create_dir(&stale)?;
    created.push(stale.clone());

    let counter = AtomicU64::new(0);
    let first = unique_temp_dir_in_with_counter(&parent, &prefix, &counter)?;
    assert_eq!(first, parent.join(format!("{prefix}-{process_id}-1")));
    created.push(first);

    let counter = std::sync::Arc::new(counter);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(PARALLEL_CALLS));
    let handles = (0..PARALLEL_CALLS)
        .map(|_| {
            let counter = std::sync::Arc::clone(&counter);
            let barrier = std::sync::Arc::clone(&barrier);
            let parent = parent.clone();
            let prefix = prefix.clone();
            std::thread::spawn(move || {
                barrier.wait();
                unique_temp_dir_in_with_counter(&parent, &prefix, &counter)
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        let path = handle
            .join()
            .map_err(|_| io::Error::other("temporary directory allocator thread panicked"))??;
        created.push(path);
    }

    let restarted = unique_temp_dir_in_with_counter(&parent, &prefix, &AtomicU64::new(0))?;
    assert!(!created.contains(&restarted));
    created.push(restarted);
    let unique = created.iter().collect::<std::collections::HashSet<_>>();
    assert_eq!(unique.len(), created.len());
    assert!(created.iter().all(|path| path.is_dir()));

    for path in created {
        std::fs::remove_dir_all(path)?;
    }
    Ok(())
}
