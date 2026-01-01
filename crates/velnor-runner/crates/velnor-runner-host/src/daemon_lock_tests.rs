use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{EngineLineageGuard, canonical_journal_path, engine_key};
use crate::error::HostError;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> io::Result<Self> {
        let path = std::env::temp_dir().join(format!("velnor-lineage-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

fn guard(engine: &str, scratch: &Scratch) -> Result<EngineLineageGuard, HostError> {
    EngineLineageGuard::acquire_at(engine, &scratch.0)
}

fn instance() -> String {
    "a".repeat(32)
}

fn verify(guard: &EngineLineageGuard, path: &Path, revision: u64) -> Result<(), HostError> {
    let path = canonical_journal_path(path)?;
    guard.verify_lineage(&path, &instance(), revision)
}

#[test]
fn engine_lock_is_shared_across_journal_paths() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let first_path = scratch.file("first.db");
    let second_path = scratch.file("second.db");
    fs::write(&first_path, b"database")?;
    fs::write(&second_path, b"database")?;
    let first = guard("engine-a", &scratch)?;
    verify(&first, &first_path, 0)?;
    assert!(matches!(guard("engine-a", &scratch), Err(HostError::Lock)));
    drop(first);
    let second = guard("engine-a", &scratch)?;
    assert_eq!(verify(&second, &second_path, 0), Err(HostError::Journal));
    Ok(())
}

#[test]
fn process_registry_keeps_the_engine_lock_until_exit() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let engine = format!("engine-{}", uuid::Uuid::new_v4());
    let first_path = scratch.file("first.db");
    let second_path = scratch.file("second.db");
    fs::write(&first_path, b"database")?;
    fs::write(&second_path, b"database")?;
    let first = EngineLineageGuard::acquire_process_at(&engine, &scratch.0)?;
    verify(&first, &first_path, 0)?;
    drop(first);
    assert!(matches!(
        EngineLineageGuard::acquire_at(&engine, &scratch.0),
        Err(HostError::Lock)
    ));
    let later = EngineLineageGuard::acquire_process_at(&engine, &scratch.0)?;
    verify(&later, &first_path, 1)?;
    assert_eq!(verify(&later, &second_path, 0), Err(HostError::Journal));
    Ok(())
}

#[test]
fn copied_database_and_same_instance_cannot_change_paths() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let original = scratch.file("launch.db");
    let copied = scratch.file("restored.db");
    fs::write(&original, b"same database bytes")?;
    fs::copy(&original, &copied)?;
    let first = guard("engine-a", &scratch)?;
    verify(&first, &original, 7)?;
    drop(first);
    let restored = guard("engine-a", &scratch)?;
    assert_eq!(verify(&restored, &copied, 7), Err(HostError::Journal));
    Ok(())
}

#[cfg(unix)]
#[test]
fn rollback_on_the_same_inode_fails_closed() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::MetadataExt;

    let scratch = Scratch::new()?;
    let database = scratch.file("launch.db");
    fs::write(&database, b"journal")?;
    let inode = fs::metadata(&database)?.ino();
    let first = guard("engine-a", &scratch)?;
    verify(&first, &database, 12)?;
    drop(first);
    let restarted = guard("engine-a", &scratch)?;
    assert_eq!(verify(&restarted, &database, 11), Err(HostError::Journal));
    assert_eq!(fs::metadata(&database)?.ino(), inode);
    Ok(())
}

#[test]
fn anchor_from_another_engine_is_rejected() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let database = scratch.file("launch.db");
    fs::write(&database, b"journal")?;
    let first = guard("engine-a", &scratch)?;
    verify(&first, &database, 3)?;
    drop(first);
    let source = scratch
        .0
        .join(format!("engine-{}.anchor", engine_key("engine-a")));
    let target = scratch
        .0
        .join(format!("engine-{}.anchor", engine_key("engine-b")));
    fs::copy(source, target)?;
    let changed_engine = guard("engine-b", &scratch)?;
    assert_eq!(
        verify(&changed_engine, &database, 3),
        Err(HostError::Journal)
    );
    Ok(())
}

#[test]
fn restart_and_one_step_crash_recovery_preserve_the_anchor() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let database = scratch.file("launch.db");
    fs::write(&database, b"journal")?;
    let first = guard("engine-a", &scratch)?;
    verify(&first, &database, 0)?;
    drop(first);
    let before_commit = guard("engine-a", &scratch)?;
    verify(&before_commit, &database, 0)?;
    drop(before_commit);
    let after_commit = guard("engine-a", &scratch)?;
    verify(&after_commit, &database, 1)?;
    drop(after_commit);
    let restarted = guard("engine-a", &scratch)?;
    verify(&restarted, &database, 1)?;
    Ok(())
}

#[test]
fn skipped_revision_or_partial_anchor_fails_closed() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let database = scratch.file("launch.db");
    fs::write(&database, b"journal")?;
    let first = guard("engine-a", &scratch)?;
    verify(&first, &database, 4)?;
    assert_eq!(first.advance_revision(6), Err(HostError::Journal));
    drop(first);
    let anchor = scratch
        .0
        .join(format!("engine-{}.anchor", engine_key("engine-a")));
    fs::write(anchor, b"{\"version\":")?;
    let restarted = guard("engine-a", &scratch)?;
    assert_eq!(verify(&restarted, &database, 5), Err(HostError::Journal));
    Ok(())
}

#[test]
fn partial_temporary_anchor_recovers_only_one_database_commit() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let database = scratch.file("launch.db");
    fs::write(&database, b"journal")?;
    let first = guard("engine-a", &scratch)?;
    verify(&first, &database, 8)?;
    drop(first);
    let anchor_name = format!("engine-{}.anchor", engine_key("engine-a"));
    fs::write(
        scratch.file(&format!(".{anchor_name}.tmp-interrupted")),
        b"partial",
    )?;
    let recovered = guard("engine-a", &scratch)?;
    verify(&recovered, &database, 9)?;
    assert_eq!(recovered.advance_revision(11), Err(HostError::Journal));
    drop(recovered);
    let restarted = guard("engine-a", &scratch)?;
    verify(&restarted, &database, 9)?;
    Ok(())
}

#[test]
fn initial_partial_anchor_cannot_be_reinitialized() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let database = scratch.file("launch.db");
    fs::write(&database, b"journal")?;
    let anchor_name = format!("engine-{}.anchor", engine_key("engine-a"));
    fs::write(
        scratch.file(&format!(".{anchor_name}.tmp-interrupted")),
        b"partial",
    )?;
    let restarted = guard("engine-a", &scratch)?;
    assert_eq!(verify(&restarted, &database, 0), Err(HostError::Journal));
    assert!(!scratch.file(&anchor_name).exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn relative_symlink_and_symlinked_anchor_roots_are_rejected() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let scratch = Scratch::new()?;
    assert_eq!(
        canonical_journal_path(Path::new("relative.db")),
        Err(HostError::Path)
    );
    assert_eq!(
        canonical_journal_path(&scratch.file("bad\nname.db")),
        Err(HostError::Path)
    );
    let actual = scratch.file("actual.db");
    let alias = scratch.file("alias.db");
    fs::write(&actual, b"journal")?;
    symlink(&actual, &alias)?;
    assert_eq!(canonical_journal_path(&alias), Err(HostError::Path));
    let root = scratch.file("real-root");
    let root_alias = scratch.file("root-alias");
    fs::create_dir(&root)?;
    symlink(&root, &root_alias)?;
    assert!(matches!(
        EngineLineageGuard::acquire_at("engine-a", &root_alias),
        Err(HostError::Path)
    ));
    let anchor_root = scratch.file("anchor-root");
    let database = scratch.file("journal.db");
    let outside = scratch.file("outside");
    fs::create_dir(&anchor_root)?;
    fs::write(&database, b"journal")?;
    fs::write(&outside, b"anchor")?;
    let anchor = anchor_root.join(format!("engine-{}.anchor", engine_key("engine-b")));
    symlink(&outside, &anchor)?;
    let restarted = EngineLineageGuard::acquire_at("engine-b", &anchor_root)?;
    assert_eq!(verify(&restarted, &database, 0), Err(HostError::Path));
    Ok(())
}
