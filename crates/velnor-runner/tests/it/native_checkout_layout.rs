// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use anyhow::Result;
use url::Url;

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, commit, finish, git};

pub(crate) fn mirror_in(store: &Path, server_url: &str, repository_id: u64) -> PathBuf {
    let Ok(url) = Url::parse(server_url) else {
        return store.join("git/invalid");
    };
    let identity = format!(
        "{}://{}{}",
        url.scheme(),
        url.host_str().unwrap_or_default(),
        url.path()
    );
    let key: String = identity
        .bytes()
        .map(|byte| match byte {
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'-' | b'_' | b'.' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect();
    store
        .join("git")
        .join(key)
        .join(format!("tailrocks/velnor-new/{repository_id}.git"))
}

#[test]
fn servers_that_differ_only_by_path_get_separate_mirrors() -> Result<()> {
    let rig = Rig::new()?;
    let other = rig.root.join("up2/tailrocks/velnor-new.git");
    let work = rig.root.join("other-work");
    std::fs::create_dir_all(&other)?;
    std::fs::create_dir_all(&work)?;
    git(&other, &["init", "-q", "--bare"])?;
    git(&work, &["init", "-q", "-b", "main"])?;
    let theirs = commit(&work, "theirs")?;
    git(&work, &["push", "-q", &other.to_string_lossy(), "main"])?;
    let second = format!("file://{}", rig.root.join("up2").display());
    let first = rig.server_url();
    for (url, sha) in [(&first, &rig.head), (&second, &theirs)] {
        let job = rig.job("checkout_depth1", sha, url)?;
        let scratch = Scratch::new()?;
        let ran = finish(rig.start(&job, &scratch)?)?;
        assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
        assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, *sha);
    }
    let mirrors = [
        mirror_in(&rig.store(), &first, 1),
        mirror_in(&rig.store(), &second, 1),
    ];
    assert_ne!(mirrors[0], mirrors[1]);
    git(&mirrors[0], &["cat-file", "-e", &rig.head])?;
    assert!(git(&mirrors[0], &["cat-file", "-e", &theirs]).is_err());
    git(&mirrors[1], &["cat-file", "-e", &theirs])?;
    assert!(git(&mirrors[1], &["cat-file", "-e", &rig.head]).is_err());
    Ok(())
}

#[test]
fn a_mirror_whose_object_database_vanished_is_initialised_again() -> Result<()> {
    let rig = Rig::new()?;
    let warm = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    std::fs::remove_dir_all(rig.mirror().join("objects"))?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    assert_eq!(rig.fetches(), 2);
    Ok(())
}

#[test]
fn a_mirror_whose_refs_directory_vanished_is_initialised_again() -> Result<()> {
    let rig = Rig::new()?;
    let warm = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    let refs = rig.mirror().join("refs");
    std::fs::remove_dir_all(&refs)?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    assert!(refs.is_dir());
    git(&rig.mirror(), &["for-each-ref"])?;
    Ok(())
}

#[test]
fn the_test_git_helper_never_finds_a_repository_above_its_directory() -> Result<()> {
    let rig = Rig::new()?;
    let inner = rig.root.join("work/inner");
    std::fs::create_dir_all(&inner)?;
    assert!(git(&rig.root.join("work"), &["rev-parse", "HEAD"]).is_ok());
    assert!(git(&inner, &["rev-parse", "HEAD"]).is_err());
    Ok(())
}
