// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{TempDir, error_text, expected_digest, text};
use anyhow::Result;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

const FIFO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Trap {
    fired: AtomicBool,
    failure: Mutex<Option<String>>,
}

impl Trap {
    fn armed(
        target: PathBuf,
        swap: impl Fn() -> io::Result<()> + Send + Sync + 'static,
    ) -> (Arc<Self>, impl Fn(&Path) + Send + Sync + 'static) {
        let trap = Arc::new(Self::default());
        let shared = Arc::clone(&trap);
        let hook = move |path: &Path| {
            if path != target || shared.fired.swap(true, Ordering::SeqCst) {
                return;
            }
            if let Err(error) = swap() {
                *shared
                    .failure
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(error.to_string());
            }
        };
        (trap, hook)
    }

    fn assert_swapped(&self) {
        assert!(self.fired.load(Ordering::SeqCst), "the swap never ran");
        let failure = self.failure.lock().unwrap_or_else(PoisonError::into_inner);
        assert_eq!(failure.as_deref(), None, "the swap failed");
    }
}

fn replace_with_symlink(path: &Path, target: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    std::os::unix::fs::symlink(target, path)
}

fn replace_with_pipe(path: &Path) -> io::Result<()> {
    fs::remove_file(path)?;
    if Command::new("mkfifo").arg(path).status()?.success() {
        Ok(())
    } else {
        Err(io::Error::other("mkfifo failed"))
    }
}

#[test]
fn expr_hash_files_does_not_follow_a_file_swapped_for_an_outside_link_before_it_opens() -> Result<()>
{
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/swap.txt", "kept")?;
    dir.write("outside/secret.txt", "secret")?;
    let swapped = dir.workspace()?.join("swap.txt");
    let secret = dir.path().join("outside/secret.txt");
    let mut env = dir.env();
    let (trap, hook) = Trap::armed(swapped.clone(), move || {
        replace_with_symlink(&swapped, &secret)
    });
    env.set_walk_seam(hook);

    assert_eq!(text("hashFiles('*.txt')", &env)?, expected_digest(&[b"a"]));
    trap.assert_swapped();
    Ok(())
}

#[test]
fn expr_hash_files_hashes_a_file_swapped_for_an_inside_link_through_its_target() -> Result<()> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/swap.txt", "gone")?;
    dir.write("ws/real/inner.md", "inner")?;
    let workspace = dir.workspace()?;
    let swapped = workspace.join("swap.txt");
    let target = workspace.join("real/inner.md");
    let mut env = dir.env();
    let (trap, hook) = Trap::armed(swapped.clone(), move || {
        replace_with_symlink(&swapped, &target)
    });
    env.set_walk_seam(hook);

    assert_eq!(
        text("hashFiles('*.txt')", &env)?,
        expected_digest(&[b"a", b"inner"])
    );
    trap.assert_swapped();
    Ok(())
}

#[test]
fn expr_hash_files_fails_on_a_directory_swapped_for_an_outside_link_before_it_opens() -> Result<()>
{
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/sub/b.txt", "b")?;
    dir.write("outside/secret.txt", "secret")?;
    let swapped = dir.workspace()?.join("sub");
    let outside = dir.path().join("outside");
    let mut env = dir.env();
    let (trap, hook) = Trap::armed(swapped.clone(), move || {
        replace_with_symlink(&swapped, &outside)
    });
    env.set_walk_seam(hook);

    let message = error_text("hashFiles('**/*.txt')", &env)?;

    assert!(message.contains("sub:"), "{message}");
    assert!(!message.contains("secret"), "{message}");
    trap.assert_swapped();
    Ok(())
}

#[test]
fn expr_hash_files_fails_on_a_link_target_swapped_for_an_outside_link_before_it_opens() -> Result<()>
{
    let dir = TempDir::new()?;
    dir.write("ws/real.txt", "real")?;
    dir.write("outside/secret.txt", "secret")?;
    let swapped = dir.workspace()?.join("real.txt");
    dir.symlink("ws/link.txt", &swapped)?;
    let secret = dir.path().join("outside/secret.txt");
    let mut env = dir.env();
    let (trap, hook) = Trap::armed(swapped.clone(), move || {
        replace_with_symlink(&swapped, &secret)
    });
    env.set_walk_seam(hook);

    let message = error_text("hashFiles('link.txt')", &env)?;

    assert!(message.contains("real.txt:"), "{message}");
    trap.assert_swapped();
    Ok(())
}

#[test]
fn expr_hash_files_reports_a_file_swapped_for_a_pipe_before_it_opens_without_blocking() -> Result<()>
{
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/swap.txt", "gone")?;
    let swapped = dir.workspace()?.join("swap.txt");
    let mut env = dir.env();
    env.hash_files_timeout = FIFO_TIMEOUT;
    let pipe = swapped.clone();
    let (trap, hook) = Trap::armed(swapped, move || replace_with_pipe(&pipe));
    env.set_walk_seam(hook);

    let message = error_text("hashFiles('*.txt')", &env)?;

    assert!(
        message.ends_with("swap.txt: not a regular file"),
        "{message}"
    );
    trap.assert_swapped();
    Ok(())
}
