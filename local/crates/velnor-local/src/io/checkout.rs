// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use tokio::fs;

use super::env::{program, words};
use crate::scenario::link_inside;

const GIT_CONFIG: &str =
    "-c user.name=velnor-local -c user.email=velnor-local@localhost -c commit.gpgsign=false";

pub(super) async fn git(repo: &Path, args: &[&str], input: Option<&[u8]>) -> io::Result<Vec<u8>> {
    let repo = repo.to_string_lossy();
    let head = [&["-C", repo.as_ref()], &words(GIT_CONFIG)[..]].concat();
    program("git", &[&head[..], args].concat(), input).await
}

pub(super) async fn copy_checkout(checkout: &Path, copy: &Path, default: &str) -> io::Result<()> {
    if fs::metadata(copy).await.is_ok() {
        fs::remove_dir_all(copy).await?;
    }
    git(
        checkout,
        &["clone", "-q", ".", &copy.to_string_lossy()],
        None,
    )
    .await?;
    let changes = git(checkout, &["diff", "HEAD", "--binary"], None).await?;
    if !changes.is_empty() {
        git(copy, &["apply", "--index"], Some(&changes)).await?;
    }
    let untracked = git(
        checkout,
        &["ls-files", "-z", "--others", "--exclude-standard"],
        None,
    )
    .await?;
    for path in untracked
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = Path::new(std::ffi::OsStr::from_bytes(path));
        copy_untracked(checkout, copy, path).await?;
    }
    if !changes.is_empty() || !untracked.is_empty() {
        git(copy, &["add", "-A"], None).await?;
        if git(copy, &["diff", "--cached", "--quiet"], None)
            .await
            .is_err()
        {
            git(copy, &["commit", "-q", "-m", "Uncommitted changes"], None).await?;
        }
    }
    git(copy, &["checkout", "-q", "-B", default], None).await?;
    Ok(())
}

pub async fn copy_untracked(checkout: &Path, copy: &Path, path: &Path) -> io::Result<()> {
    let (source, target) = (checkout.join(path), copy.join(path));
    if path.as_os_str().as_bytes().ends_with(b"/") {
        eprintln!(
            "velnor-local: {} is a nested git repository; it is left out, as git add leaves it out",
            path.display()
        );
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).await?;
    }
    if !fs::symlink_metadata(&source).await?.is_symlink() {
        fs::copy(&source, &target).await?;
        return Ok(());
    }
    let link = fs::read_link(&source).await?;
    let resolved = match fs::canonicalize(&source).await {
        Ok(real) => real.starts_with(fs::canonicalize(checkout).await?),
        Err(_) => true,
    };
    if !resolved || !link_inside(path, &link) {
        return Err(io::Error::other(format!(
            "{} is a link to {}, which leaves the checkout",
            path.display(),
            link.display()
        )));
    }
    fs::symlink(&link, &target).await
}
