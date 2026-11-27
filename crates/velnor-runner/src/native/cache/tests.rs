// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;

use anyhow::Result;
use velnor_cache_rules::{Access, Scope};

use super::*;
use crate::broker::TrustedFacts;
use crate::broker::cache::{DATA, MARKER, entry_dir};

const FACTS: &str = r#"{"repository_id": 4242, "repository": "tailrocks/velnor-new",
    "server_url": "https://github.com", "default_branch": "main",
    "sha": "0123456789abcdef0123456789abcdef01234567", "run_id": 1,
    "run_attempt": 1, "event_name": "push",
    "event": {"kind": "branch", "branch": "main"}}"#;

fn plan(root: PathBuf) -> Result<Plan> {
    let scope = Scope::branch("main");
    Ok(Plan {
        root,
        owner: CacheOwner::of(&TrustedFacts::parse(FACTS.as_bytes())?),
        access: CacheAccess {
            rules: Access {
                read: vec![scope.clone()],
                write: scope,
            },
            mode: CacheMode::Read,
        },
        roots: vec![std::env::temp_dir().join("velnor-never-restored")],
        umask: Umask::read(),
        version: sha256_hex("paths"),
        inputs: CacheInputs {
            paths: vec!["data".to_owned()],
            key: "k-1".to_owned(),
            restore_keys: Vec::new(),
            lookup_only: false,
        },
    })
}

struct Locked {
    root: PathBuf,
    directory: PathBuf,
}

impl Locked {
    fn new(root: PathBuf, directory: PathBuf) -> Result<Self> {
        fs::create_dir_all(&directory)?;
        Ok(Self { root, directory })
    }

    fn lock(&self) -> Result<()> {
        fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o000))?;
        Ok(())
    }
}

impl Drop for Locked {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700));
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn scratch_root() -> PathBuf {
    std::env::temp_dir().join(format!("velnor-vanished-{}", uuid::Uuid::new_v4()))
}

fn assert_missed_with_a_warning(done: &Done) {
    assert_eq!(done.lines.len(), 2, "{:?}", done.lines);
    assert!(done.lines[0].starts_with("::warning::Failed to restore:"));
    assert!(done.lines[0].contains("vanished"));
    assert!(done.lines[1].starts_with("Cache not found for input keys"));
    assert_eq!(done.outputs.len(), 0);
    assert!(done.hit.is_none());
}

fn hit_in(dir: PathBuf) -> store::CacheEntry {
    store::CacheEntry {
        scope: Scope::branch("main"),
        version: sha256_hex("paths"),
        key: "k-1".to_owned(),
        dir,
    }
}

#[test]
fn an_entry_that_vanishes_before_it_is_opened_is_a_miss_with_a_warning() -> Result<()> {
    let root = scratch_root();
    let done = restore_hit(&plan(root.clone())?, hit_in(root.join("gone")), None)?;
    assert_missed_with_a_warning(&done);
    Ok(())
}

#[test]
fn an_entry_the_step_may_not_open_fails_the_restore_instead_of_missing() -> Result<()> {
    let root = scratch_root();
    let dir = root.join("entry");
    let locked = Locked::new(root.clone(), dir.clone())?;
    fs::write(dir.join(DATA), b"")?;
    locked.lock()?;
    let error = restore_hit(&plan(root)?, hit_in(dir), None)
        .err()
        .map(|error| format!("{error:#}"));
    assert!(
        error
            .as_deref()
            .is_some_and(|text| text.contains("as a regular file")),
        "{error:?}"
    );
    Ok(())
}

#[test]
fn a_lookup_fails_on_an_entry_the_step_may_not_read_instead_of_skipping_it() -> Result<()> {
    let root = scratch_root();
    let plan = plan(root.clone())?;
    let dir = entry_dir(
        &root,
        &plan.owner,
        &Scope::branch("main"),
        &plan.version,
        "k-1",
    );
    let locked = Locked::new(root.clone(), dir.clone())?;
    fs::write(dir.join(MARKER), b"")?;
    locked.lock()?;
    let found = store::find(
        &root,
        &plan.owner,
        &plan.access.rules,
        "k-1",
        &[],
        &plan.version,
        None,
    );
    let error = found.err().map(|error| format!("{error:#}"));
    assert!(
        error.as_deref().is_some_and(|text| text.contains(MARKER)),
        "{error:?}"
    );
    Ok(())
}

#[test]
fn a_lookup_that_ends_after_the_job_was_cancelled_is_cancelled_not_a_miss() -> Result<()> {
    let root = scratch_root();
    let cancel = JobCancel::new();
    cancel.cancel();
    let error = restore(&plan(root)?, Some(&cancel)).err();
    assert!(error.is_some_and(|error| error.is::<archive::Cancelled>()));
    Ok(())
}
