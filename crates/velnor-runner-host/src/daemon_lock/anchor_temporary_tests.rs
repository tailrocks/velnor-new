// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use super::{Anchor, EngineLineageGuard, write_anchor_with};
use crate::error::HostError;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const ENGINE: &str = "engine-a";
const INSTANCE: &str = "0123456789abcdef0123456789abcdef";

struct Fixture {
    _state: tempfile::TempDir,
    root: PathBuf,
    journal: PathBuf,
    guard: EngineLineageGuard,
}

fn fixture() -> TestResult<Fixture> {
    let state = tempfile::tempdir()?;
    let root = fs::canonicalize(state.path())?.join("lineage");
    let journal = fs::canonicalize(state.path())?.join("journal.sqlite");
    let guard = EngineLineageGuard::acquire_at(ENGINE, &root)?;
    let root = fs::canonicalize(&root)?;
    Ok(Fixture {
        _state: state,
        root,
        journal,
        guard,
    })
}

fn anchor_path(fixture: &Fixture) -> PathBuf {
    fixture.guard.inner.anchor_path.clone()
}

fn temporaries(root: &Path) -> TestResult<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.contains(".tmp-") {
            names.push(name);
        }
    }
    Ok(names)
}

fn stale_name(anchor: &Path, suffix: &str) -> TestResult<String> {
    let name = anchor
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("anchor name")?;
    Ok(format!(".{name}.tmp-{suffix}"))
}

#[test]
fn a_stale_anchor_temporary_does_not_block_the_first_bind() -> TestResult<()> {
    let fixture = fixture()?;
    let anchor = anchor_path(&fixture);
    let stale = fixture.root.join(stale_name(&anchor, "stale")?);
    fs::write(&stale, b"{\"partial\"")?;

    fixture
        .guard
        .verify_lineage(&fixture.journal, INSTANCE, 0)?;

    assert!(anchor.is_file());
    assert!(!stale.exists());
    assert_eq!(temporaries(&fixture.root)?, Vec::<String>::new());
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_stale_temporary_symlink_is_unlinked_without_touching_its_target() -> TestResult<()> {
    let fixture = fixture()?;
    let anchor = anchor_path(&fixture);
    let victim = fixture.root.join("victim");
    fs::write(&victim, b"keep")?;
    let stale = fixture.root.join(stale_name(&anchor, "link")?);
    std::os::unix::fs::symlink(&victim, &stale)?;

    fixture
        .guard
        .verify_lineage(&fixture.journal, INSTANCE, 0)?;

    assert_eq!(fs::read(&victim)?, b"keep");
    assert!(fs::symlink_metadata(&stale).is_err());
    Ok(())
}

#[test]
fn a_published_anchor_is_never_removed_with_stale_temporaries() -> TestResult<()> {
    let fixture = fixture()?;
    fixture
        .guard
        .verify_lineage(&fixture.journal, INSTANCE, 0)?;
    let anchor = anchor_path(&fixture);
    let published = fs::read(&anchor)?;

    let second = EngineLineageGuard::acquire_at("engine-b", &fixture.root)?;
    let other_anchor = second.inner.anchor_path.clone();
    fs::write(fixture.root.join(stale_name(&other_anchor, "stale")?), b"x")?;
    second.verify_lineage(&fixture.journal, INSTANCE, 0)?;

    assert_eq!(fs::read(&anchor)?, published);
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_symlink_at_the_anchor_path_still_fails() -> TestResult<()> {
    let fixture = fixture()?;
    let anchor = anchor_path(&fixture);
    let target = fixture.root.join("target");
    fs::write(&target, b"{}")?;
    std::os::unix::fs::symlink(&target, &anchor)?;
    fs::write(fixture.root.join(stale_name(&anchor, "stale")?), b"x")?;

    assert!(
        fixture
            .guard
            .verify_lineage(&fixture.journal, INSTANCE, 0)
            .is_err()
    );
    assert_eq!(fs::read(&target)?, b"{}");
    Ok(())
}

#[test]
fn a_failed_publish_leaves_no_temporary_behind() -> TestResult<()> {
    let fixture = fixture()?;
    let anchor = anchor_path(&fixture);
    let desired = Anchor {
        version: super::ANCHOR_VERSION,
        engine_id: ENGINE.to_owned(),
        journal_path: fixture.journal.to_string_lossy().into_owned(),
        instance_id: INSTANCE.to_owned(),
        revision: 0,
    };

    let failed = write_anchor_with(&anchor, &desired, |_, _| {
        Err(std::io::Error::other("rename failed"))
    });

    assert!(matches!(failed, Err(HostError::Journal)));
    assert_eq!(temporaries(&fixture.root)?, Vec::<String>::new());
    assert!(!anchor.exists());
    fixture
        .guard
        .verify_lineage(&fixture.journal, INSTANCE, 0)?;
    Ok(())
}
