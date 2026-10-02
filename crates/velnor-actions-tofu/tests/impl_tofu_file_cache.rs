//! Snapshot-scoped read-cache cases: memoization, failure fidelity,
//! and the oversize read-through bound.
use velnor_actions_tofu::FileCache;
use velnor_actions_tofu::parser::MAX_FILE_BYTES;

use crate::support::{Outcome, TempDir};

#[test]
fn second_read_serves_cached_bytes_after_deletion() -> Outcome {
    let dir = TempDir::create("tofu-read-cache")?;
    let path = dir.write("main.tf", "variable \"a\" {}\n")?;
    let mut reads = FileCache::new();
    let first = reads.read_raw(&path).map_err(|err| err.to_string())?;
    std::fs::remove_file(&path)?;
    let second = reads.read_raw(&path).map_err(|err| err.to_string())?;
    assert_eq!(first, second, "cache serves the deleted file's bytes");
    assert_eq!(second, b"variable \"a\" {}\n");
    Ok(())
}

#[test]
fn cached_failures_keep_kind_and_message() -> Outcome {
    let dir = TempDir::create("tofu-read-cache-miss")?;
    let missing = dir.path().join("absent.tf");
    let mut reads = FileCache::new();
    let first = reads
        .read_raw(&missing)
        .expect_err("missing file fails live");
    assert_eq!(first.kind(), std::io::ErrorKind::NotFound);
    let message = first.to_string();
    let second = reads
        .read_raw(&missing)
        .expect_err("missing file fails cached");
    assert_eq!(second.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(second.to_string(), message, "rebuilt message matches");
    Ok(())
}

#[test]
fn oversize_files_read_through_uncached() -> Outcome {
    let dir = TempDir::create("tofu-read-cache-big")?;
    let bound = usize::try_from(MAX_FILE_BYTES).map_err(|err| err.to_string())?;
    let body = "#".repeat(bound + 1);
    let path = dir.write("big.tf", &body)?;
    let mut reads = FileCache::new();
    let first = reads.read_raw(&path).map_err(|err| err.to_string())?;
    assert_eq!(first.len(), body.len());
    std::fs::remove_file(&path)?;
    assert!(
        reads.read_raw(&path).is_err(),
        "oversize second read goes live"
    );
    Ok(())
}
