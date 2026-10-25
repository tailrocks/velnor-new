use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::context::{FreshnessContext, RUST_SOURCE_CAP, parse_iso_date, parse_timestamp};

const SOURCE_FIXTURE_ATTEMPTS: usize = 32;
static NEXT_SOURCE_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

fn source_fixture_root(process_id: u32, suffix: u128) -> PathBuf {
    std::env::temp_dir().join(format!("velnor-freshness-source-{process_id}-{suffix}"))
}

fn source_fixture_with_ids(
    source: &[u8],
    process_id: u32,
    mut next_id: impl FnMut() -> Option<u128>,
) -> Result<(PathBuf, FreshnessContext), String> {
    for _ in 0..SOURCE_FIXTURE_ATTEMPTS {
        let suffix = next_id().ok_or_else(|| "fixture identifier exhausted".to_owned())?;
        let root = source_fixture_root(process_id, suffix);
        match fs::create_dir(&root) {
            Ok(()) => {
                let pin_path = root.join("pins.rs");
                if let Err(write_error) = fs::write(&pin_path, source) {
                    let failure = format!(
                        "fixture write failed for {}: {write_error}",
                        pin_path.display()
                    );
                    return match fs::remove_dir_all(&root) {
                        Ok(()) => Err(failure),
                        Err(cleanup_error) => Err(format!(
                            "{failure}; fixture cleanup failed for {}: {cleanup_error}",
                            root.display()
                        )),
                    };
                }
                return Ok((root.clone(), FreshnessContext::new(root, false, false)));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(format!(
                    "fixture directory creation failed for {}: {error}",
                    root.display()
                ));
            }
        }
    }
    Err(format!(
        "fixture allocation exhausted {SOURCE_FIXTURE_ATTEMPTS} attempts"
    ))
}

fn source_fixture(source: &[u8]) -> Result<(PathBuf, FreshnessContext), String> {
    source_fixture_with_ids(source, std::process::id(), reserve_source_fixture_id)
}

fn reserve_source_fixture_id() -> Option<u128> {
    let mut current = NEXT_SOURCE_FIXTURE_ID.load(Ordering::Relaxed);
    loop {
        let next = current.checked_add(1)?;
        match NEXT_SOURCE_FIXTURE_ID.compare_exchange_weak(
            current,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return Some(u128::from(current)),
            Err(observed) => current = observed,
        }
    }
}

fn remove_source_fixture(root: &Path) -> Result<(), String> {
    fs::remove_dir_all(root)
        .map_err(|error| format!("fixture cleanup failed for {}: {error}", root.display()))
}

fn check_source(source: &[u8]) -> (Option<String>, Vec<String>) {
    let (root, mut context) = match source_fixture(source) {
        Ok(fixture) => fixture,
        Err(error) => return (None, vec![error]),
    };
    let value = context.rust_const("pins.rs", "PIN");
    let failures = context.failures.clone();
    let mut failures = failures;
    if let Err(error) = remove_source_fixture(&root) {
        failures.push(error);
    }
    (value, failures)
}

fn check_usize_source(source: &[u8]) -> (Option<String>, Vec<String>) {
    let (root, mut context) = match source_fixture(source) {
        Ok(fixture) => fixture,
        Err(error) => return (None, vec![error]),
    };
    let value = context.rust_usize_const("pins.rs", "PIN");
    let failures = context.failures.clone();
    let mut failures = failures;
    if let Err(error) = remove_source_fixture(&root) {
        failures.push(error);
    }
    (value, failures)
}

