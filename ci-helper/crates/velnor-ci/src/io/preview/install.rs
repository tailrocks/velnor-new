// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Instant;

use nix::sys::signal::{SigSet, SigmaskHow, Signal, pthread_sigmask};
use nix::unistd::geteuid;

use crate::error::CiError;
use crate::preview::json::{Res, hex, require};
use crate::preview::runtime::Verified;

const FINAL_NAME: &str = "source-preview-runtime";

fn error(what: &str) -> CiError {
    CiError::new(format!("source preview install: {what}"))
}

/// Port of `check_owned_directory`: a directory of this user that nobody else may write.
fn check_owned_directory(path: &Path) -> Res<()> {
    let meta = fs::symlink_metadata(path).map_err(|_| error("directory"))?;
    require(meta.is_dir() && meta.uid() == geteuid().as_raw() && meta.mode() & 0o022 == 0)
}

/// Move `from` to `to` only if nothing is at `to`: `mkdir` claims the name atomically
/// and fails when it exists, and the rename of a directory over an empty directory is
/// atomic, so `to` appears complete or not at all.
fn rename_exclusive(from: &Path, to: &Path) -> Res<()> {
    DirBuilder::new()
        .mode(0o700)
        .create(to)
        .map_err(|_| error("rename"))?;
    fs::rename(from, to).map_err(|_| {
        drop(fs::remove_dir(to));
        error("rename")
    })
}

struct HeldSignals(SigSet);

impl HeldSignals {
    fn hold() -> Res<Self> {
        let mut held = SigSet::empty();
        for signal in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGALRM] {
            held.add(signal);
        }
        let mut previous = SigSet::empty();
        pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&held), Some(&mut previous))
            .map_err(|_| error("signals"))?;
        Ok(Self(previous))
    }
}

impl Drop for HeldSignals {
    fn drop(&mut self) {
        pthread_sigmask(SigmaskHow::SIG_SETMASK, Some(&self.0), None).unwrap_or_default();
    }
}

pub(super) fn random_name() -> Res<String> {
    use std::io::Read;
    let mut bytes = [0_u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|_| error("random"))?;
    Ok(hex(&bytes))
}

fn write_all(directory: &Path, name: &str, data: &[u8], deadline: Instant) -> Res<()> {
    require(Instant::now() < deadline)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(directory.join(name))
        .map_err(|_| error("create"))?;
    file.write_all(data).map_err(|_| error("write"))?;
    file.sync_all().map_err(|_| error("sync"))
}

/// Port of `GITHUB_OUTPUT` handling of the legacy step: open the file without following
/// a link and write both lines with one `write`, only when it is a regular, empty file
/// of this user with no other link.
pub fn write_outputs(path: &Path, text: &str) -> Res<()> {
    let mut file = OpenOptions::new()
        .append(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| error("output"))?;
    let meta = file.metadata().map_err(|_| error("output"))?;
    require(
        meta.is_file() && meta.uid() == geteuid().as_raw() && meta.nlink() == 1 && meta.len() == 0,
    )?;
    file.write_all(text.as_bytes()).map_err(|_| error("output"))
}

/// Port of `install`: write the SDK files into a private directory of `root`, then
/// rename it to `source-preview-runtime` without replacing anything, then run `emit`.
/// A failure at any point removes what was made. Returns the installed directory.
pub fn install(
    root: &Path,
    verified: &Verified,
    deadline: Instant,
    emit: impl FnOnce() -> Res<()>,
) -> Res<PathBuf> {
    require(root.is_absolute() && fs::canonicalize(root).ok().as_deref() == Some(root))?;
    check_owned_directory(root)?;
    let temporary = root.join(format!(".source-preview-runtime-{}", random_name()?));
    DirBuilder::new()
        .mode(0o700)
        .create(&temporary)
        .map_err(|_| error("mkdir"))?;
    let created = fs::symlink_metadata(&temporary).map_err(|_| error("mkdir"))?;
    let committed = root.join(FINAL_NAME);
    let mut current = temporary.clone();
    let mut held = None;
    let result = (|| -> Res<()> {
        let files: [(&str, &[u8]); 4] = [
            ("linux", &verified.release.linux),
            ("darwin", &verified.release.darwin),
            ("receipt.json", &verified.release.receipt),
            ("bundle.json", &verified.release.bundle),
        ];
        for (name, data) in files {
            write_all(&temporary, name, data, deadline)?;
        }
        write_all(&temporary, "identity.json", &verified.identity(), deadline)?;
        File::open(&temporary)
            .and_then(|d| d.sync_all())
            .map_err(|_| error("sync"))?;
        require(Instant::now() < deadline)?;
        held = Some(HeldSignals::hold()?);
        rename_exclusive(&temporary, &committed)?;
        current.clone_from(&committed);
        require(Instant::now() < deadline)?;
        emit()
    })();
    if result.is_err() {
        let now = fs::symlink_metadata(&current).ok();
        if now.is_some_and(|m| (m.dev(), m.ino()) == (created.dev(), created.ino())) {
            drop(fs::remove_dir_all(&current));
        }
    }
    drop(held);
    result.map(|()| committed)
}
