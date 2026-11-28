// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use rustix::fs::{AtFlags, FlockOperation, flock, unlinkat};

use super::Broker;
use super::layout::Layout;
use super::live::{Holder, Scan, holder, open_live, prepare, retire};
use super::proto::Response;
use super::safe_fs::open_path;
use super::tests::{BRANCH, Fixture, Scratch, TAR, begin, facts_of, published, version, write_all};

fn id_of(layout: &Layout) -> Result<String> {
    layout
        .incoming()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| anyhow!("the incoming path has no name"))
}

fn trees(root: &Path, id: &str) -> Vec<PathBuf> {
    ["incoming", "aside", "live"]
        .iter()
        .map(|parent| root.join(parent).join(id))
        .filter(|path| fs::symlink_metadata(path).is_ok())
        .collect()
}

fn lock_taken(path: &Path) -> bool {
    let Ok(file) = fs::OpenOptions::new().read(true).write(true).open(path) else {
        return false;
    };
    flock(&file, FlockOperation::NonBlockingLockExclusive).is_err()
}

#[test]
fn a_new_broker_reclaims_the_trees_of_a_dead_one() -> Result<()> {
    let fixture = Fixture::new()?;
    let root = fixture.root();
    let dead = Layout::new(root.clone());
    let name = dead.next_name();
    let (upload, mut data) = dead.create_incoming(&name)?;
    data.write_all(TAR)?;
    dead.open_aside()?;
    let dead_id = id_of(&dead)?;
    drop((upload, data, dead));
    let orphan = root.join("incoming").join("orphan").join("0");
    fs::create_dir_all(&orphan)?;
    fs::write(orphan.join("data.tar"), TAR)?;
    assert_eq!(trees(&root, &dead_id).len(), 3);
    let _next = Broker::new(fixture.facts.clone(), root.clone());
    assert_eq!(trees(&root, &dead_id), Vec::<PathBuf>::new());
    assert_eq!(trees(&root, "orphan"), Vec::<PathBuf>::new());
    let own = id_of(&fixture.broker.layout)?;
    assert!(root.join("live").join(own).exists());
    Ok(())
}

#[tokio::test]
async fn a_new_broker_leaves_the_trees_of_a_live_one_alone() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut stream = begin(&fixture.broker, "k-1").await?;
    let aside = fixture.broker.layout.open_aside()?;
    let own = id_of(&fixture.broker.layout)?;
    let _next = Broker::new(fixture.facts.clone(), fixture.root());
    assert_eq!(trees(&fixture.root(), &own).len(), 3);
    drop(aside);
    let count = write_all(&mut stream, TAR.to_vec()).await?;
    let response = stream.seal(count).await;
    assert!(published(&response), "{response:?}");
    Ok(())
}

#[test]
fn a_reclaimer_that_lists_a_claim_in_progress_cannot_win_it() -> Result<()> {
    let scratch = Scratch::new()?;
    let root = scratch.0.join("store");
    let live = open_live(&root)?;
    let claiming = prepare(&live, "claimed")?;
    Scan::of(&root)
        .ok_or_else(|| anyhow!("the store has no liveness directory"))?
        .reap("reclaimer");
    let file = claiming.complete()?;
    assert!(lock_taken(&root.join("live").join("claimed")));
    drop(file);
    Ok(())
}

#[test]
fn a_claim_that_died_before_the_rename_is_reclaimed() -> Result<()> {
    let scratch = Scratch::new()?;
    let root = scratch.0.join("store");
    let live = open_live(&root)?;
    drop(prepare(&live, "crashed")?);
    assert_eq!(fs::read_dir(root.join("live"))?.count(), 1);
    Scan::of(&root)
        .ok_or_else(|| anyhow!("the store has no liveness directory"))?
        .reap("reclaimer");
    assert_eq!(fs::read_dir(root.join("live"))?.count(), 0);
    Ok(())
}

#[test]
fn a_liveness_name_replaced_after_the_lock_is_not_retired() -> Result<()> {
    let scratch = Scratch::new()?;
    let root = scratch.0.join("store");
    let live = open_live(&root)?;
    let tree = root.join("incoming").join("victim");
    fs::create_dir_all(&tree)?;
    let incoming = open_path(&root.join("incoming"))?;
    drop(prepare(&live, "victim")?.complete()?);
    let Holder::Dead(lock) = holder(&live, OsStr::new("victim")) else {
        return Err(anyhow!(
            "the liveness file of a dropped claim is still locked"
        ));
    };
    unlinkat(&live, "victim", AtFlags::empty())?;
    let successor = prepare(&live, "victim")?.complete()?;
    let retired = retire(&live, &[incoming], OsStr::new("victim"), &lock);
    assert!(!retired);
    assert!(tree.exists());
    assert!(root.join("live").join("victim").exists());
    drop(successor);
    Ok(())
}

#[tokio::test]
async fn a_broker_that_cannot_claim_its_liveness_lock_creates_no_upload() -> Result<()> {
    let scratch = Scratch::new()?;
    let root = scratch.0.join("store");
    fs::create_dir_all(&root)?;
    fs::write(root.join("live"), "x")?;
    let broker = Broker::new(facts_of(BRANCH)?, root.clone());
    let refused = broker.begin_cache("k-1", &version()).await;
    assert!(
        matches!(&refused, Err(Response::Failed { message }) if message.contains("liveness")),
        "{:?}",
        refused.err()
    );
    assert!(!root.join("incoming").exists());
    Ok(())
}

fn shared(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[tokio::test]
async fn a_shared_liveness_directory_leaves_no_broker_able_to_upload() -> Result<()> {
    let scratch = Scratch::new()?;
    let root = scratch.0.join("store");
    shared(&root.join("live"))?;
    let broker = Broker::new(facts_of(BRANCH)?, root.clone());
    let refused = broker.begin_cache("k-1", &version()).await;
    assert!(
        matches!(&refused, Err(Response::Failed { message }) if message.contains("the liveness directory is not owned")),
        "{:?}",
        refused.err()
    );
    assert!(!root.join("incoming").exists());
    Ok(())
}

#[test]
fn a_shared_aside_directory_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    shared(&fixture.root().join("aside"))?;
    let error = fixture.broker.layout.open_aside().err();
    assert!(
        error
            .is_some_and(|error| format!("{error:#}").contains("the aside directory is not owned")),
    );
    Ok(())
}

#[test]
fn a_shared_aside_directory_of_this_broker_is_refused() -> Result<()> {
    let fixture = Fixture::new()?;
    let own = id_of(&fixture.broker.layout)?;
    let aside = fixture.root().join("aside");
    fs::create_dir_all(&aside)?;
    fs::set_permissions(&aside, fs::Permissions::from_mode(0o700))?;
    shared(&aside.join(own))?;
    let error = fixture.broker.layout.open_aside().err();
    assert!(
        error.is_some_and(|error| format!("{error:#}").contains("of this broker is not owned")),
    );
    Ok(())
}
