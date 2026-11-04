// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(unix)]
mod unix {
    use crate::{OrchestratorError, internal::internal};
    use flate2::bufread::GzDecoder;
    use rustix::fs::{self, AtFlags, Mode, OFlags, RenameFlags};
    use sha2::{Digest, Sha256};
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs::File,
        io::{self, BufReader, Read, Seek, SeekFrom, Write},
        path::{Component, Path},
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, Instant},
    };

    const MAX_ARCHIVE: u64 = 1024 * 1024 * 1024;
    const MAX_TOTAL: u64 = 4 * 1024 * 1024 * 1024;
    const MAX_FILE: u64 = 256 * 1024 * 1024;
    const MAX_ENTRIES: usize = 100_000;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn invalid() -> OrchestratorError {
        internal("fd_tool_extract_invalid")
    }

    pub fn extract_verified_tool_archive_file(
        archive: &File,
        sha256: &str,
        version: &str,
        destination: &Path,
    ) -> Result<(), OrchestratorError> {
        if sha256.len() != 64
            || !sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !velnor_actions_contract::config::is_exact_semver(version)
            || !destination.is_absolute()
        {
            return Err(invalid());
        }
        let target = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => "aarch64-apple-darwin",
            ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
            _ => return Err(invalid()),
        };
        let deadline = Instant::now()
            .checked_add(Duration::from_mins(15))
            .ok_or_else(invalid)?;
        let mut input = archive.try_clone().map_err(|_| invalid())?;
        verify_digest(&mut input, sha256, deadline)?;
        let zip_deadline = velnor_actions_mise::CheckDeadline::after(
            deadline.saturating_duration_since(Instant::now()),
        )
        .map_err(|_| invalid())?;
        crate::check_runtime::preparation::acquisition::archive::preflight_zip_file(
            &mut input,
            zip_deadline,
        )?;
        preflight(&mut input, deadline).map_err(|_| internal("fd_preflight"))?;
        verify_digest(&mut input, sha256, deadline)?;
        let (parent, name) = open_parent(destination).map_err(|_| internal("fd_parent"))?;
        let (stage_name, stage) = create_stage(&parent).map_err(|_| internal("fd_stage"))?;
        let mut tree = Tree::new(stage);
        let result = extract(&mut input, &mut tree, version, target, deadline)
            .map_err(|_| internal("fd_extract"))
            .and_then(|()| verify_digest(&mut input, sha256, deadline))
            .and_then(|()| tree.finish_modes())
            .and_then(|()| promote(&parent, &stage_name, name, &tree.root));
        if result.is_err() {
            tree.cleanup()?;
            fs::unlinkat(&parent, stage_name.as_str(), AtFlags::REMOVEDIR)
                .map_err(|_| internal("fd_tool_extract_cleanup_failed"))?;
        }
        result
    }
    fn verify_digest(
        file: &mut File,
        expected: &str,
        deadline: Instant,
    ) -> Result<(), OrchestratorError> {
        let meta = file.metadata().map_err(|_| invalid())?;
        if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_ARCHIVE {
            return Err(invalid());
        }
        file.seek(SeekFrom::Start(0)).map_err(|_| invalid())?;
        let mut digest = Sha256::new();
        let mut total = 0u64;
        let mut buffer = [0u8; 16384];
        loop {
            if Instant::now() >= deadline {
                return Err(invalid());
            }
            let n = file.read(&mut buffer).map_err(|_| invalid())?;
            if n == 0 {
                break;
            }
            total = total
                .checked_add(n as u64)
                .filter(|n| *n <= MAX_ARCHIVE)
                .ok_or_else(invalid)?;
            digest.update(&buffer[..n]);
        }
        if total != meta.len()
            || crate::config::playwright_lock::hex(&digest.finalize()) != expected
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn open_parent(destination: &Path) -> Result<(File, &std::ffi::OsStr), OrchestratorError> {
        let name = destination.file_name().ok_or_else(invalid)?;
        let path = destination.parent().ok_or_else(invalid)?;
        let mut dir = File::from(
            fs::open(
                "/",
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| invalid())?,
        );
        for c in path.components() {
            match c {
                Component::RootDir => {}
                Component::Normal(n) => {
                    dir = File::from(
                        fs::openat(
                            &dir,
                            n,
                            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                            Mode::empty(),
                        )
                        .map_err(|_| invalid())?,
                    );
                    let stat = fs::fstat(&dir).map_err(|_| invalid())?;
                    let owner_ok =
                        stat.st_uid == 0 || stat.st_uid == rustix::process::geteuid().as_raw();
                    let root_sticky = stat.st_uid == 0 && stat.st_mode & 0o1000 != 0;
                    if !owner_ok || (stat.st_mode & 0o022 != 0 && !root_sticky) {
                        return Err(invalid());
                    }
                }
                _ => return Err(invalid()),
            }
        }
        let stat = fs::fstat(&dir).map_err(|_| invalid())?;
        if stat.st_uid != rustix::process::geteuid().as_raw() || stat.st_mode & 0o022 != 0 {
            return Err(invalid());
        }
        Ok((dir, name))
    }
    fn create_stage(parent: &File) -> Result<(String, File), OrchestratorError> {
        for _ in 0..128 {
            let name = format!(
                ".velnor-extract-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            match fs::mkdirat(parent, name.as_str(), Mode::from_raw_mode(0o700)) {
                Ok(()) => {
                    let Ok(fd) = fs::openat(
                        parent,
                        name.as_str(),
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    ) else {
                        fs::unlinkat(parent, name.as_str(), AtFlags::REMOVEDIR)
                            .map_err(|_| internal("fd_tool_extract_cleanup_failed"))?;
                        return Err(invalid());
                    };
                    let stat = fs::fstat(&fd).map_err(|_| invalid())?;
                    if stat.st_uid != rustix::process::geteuid().as_raw()
                        || stat.st_mode & 0o077 != 0
                    {
                        return Err(invalid());
                    }
                    return Ok((name, File::from(fd)));
                }
                Err(rustix::io::Errno::EXIST) => {}
                Err(_) => return Err(invalid()),
            }
        }
        Err(invalid())
    }
    mod tree {
        use super::{
            AtFlags, BTreeMap, BTreeSet, File, Instant, MAX_FILE, Mode, OFlags, OrchestratorError,
            Read, Write, fs, internal, invalid,
        };
        include!("fd_tool_extract_tree.rs");
    }
    use tree::Tree;
    fn extract(
        input: &mut File,
        tree: &mut Tree,
        version: &str,
        target: &str,
        deadline: Instant,
    ) -> Result<(), OrchestratorError> {
        input.seek(SeekFrom::Start(0)).map_err(|_| invalid())?;
        let mut zip = zip::ZipArchive::new(input).map_err(|_| invalid())?;
        if zip.len() != 1 {
            return Err(invalid());
        }
        let entry = zip.by_index(0).map_err(|_| invalid())?;
        if entry.name_raw() != b"rust-toolchain.tar.gz"
            || entry.is_dir()
            || entry.is_symlink()
            || entry.encrypted()
            || entry.unix_mode().is_some_and(|mode| {
                let kind = mode & 0o170_000;
                (kind != 0 && kind != 0o100_000) || mode & 0o7022 != 0
            })
            || entry.size() == 0
            || entry.size() > MAX_ARCHIVE
        {
            return Err(invalid());
        }
        let mut decoder = GzDecoder::new(BufReader::new(Timed {
            inner: entry,
            deadline,
        }));
        let mut tar = tar::Archive::new(&mut decoder);
        let mut count = 0usize;
        let mut total = 0u64;
        let prefix = format!("toolchains/{version}-{target}");
        for item in tar.entries().map_err(|_| invalid())? {
            if Instant::now() >= deadline {
                return Err(invalid());
            }
            count += 1;
            if count > MAX_ENTRIES {
                return Err(invalid());
            }
            let mut item = item.map_err(|_| invalid())?;
            let path = item
                .path()
                .map_err(|_| invalid())?
                .to_str()
                .ok_or_else(invalid)?
                .to_owned();
            let canonical = path.strip_suffix('/').unwrap_or(&path);
            if canonical != "toolchains"
                && canonical != prefix
                && !canonical.starts_with(&format!("{prefix}/"))
            {
                return Err(invalid());
            }
            let kind = item.header().entry_type();
            let directory = kind.is_dir();
            if !(directory || kind.is_file()) {
                return Err(invalid());
            }
            let size = item.size();
            total = total
                .checked_add(size)
                .filter(|n| *n <= MAX_TOTAL)
                .ok_or_else(invalid)?;
            let mode = item.header().mode().map_err(|_| invalid())?;
            tree.add(&path, directory, mode, size, &mut item, deadline)?;
        }
        let _inner = tar.into_inner();
        if count == 0 || !tree.dirs.contains(&prefix) {
            return Err(invalid());
        }
        finish_gzip(decoder, deadline)?;
        Ok(())
    }

    mod preflight {
        use super::{
            BTreeSet, BufReader, File, GzDecoder, Instant, MAX_ARCHIVE, MAX_ENTRIES, MAX_FILE,
            MAX_TOTAL, OrchestratorError, Read, Seek, SeekFrom, invalid, io,
        };
        include!("fd_tool_extract_preflight.rs");
    }
    use preflight::{Timed, finish_gzip, preflight};
    pub(super) fn promote(
        parent: &File,
        stage: &str,
        destination: &std::ffi::OsStr,
        stage_fd: &File,
    ) -> Result<(), OrchestratorError> {
        let expected = fs::fstat(stage_fd).map_err(|_| invalid())?;
        let before = fs::statat(parent, stage, AtFlags::SYMLINK_NOFOLLOW).map_err(|_| invalid())?;
        if before.st_dev != expected.st_dev || before.st_ino != expected.st_ino {
            return Err(invalid());
        }
        fs::renameat_with(parent, stage, parent, destination, RenameFlags::NOREPLACE)
            .map_err(|_| invalid())?;
        let after =
            fs::statat(parent, destination, AtFlags::SYMLINK_NOFOLLOW).map_err(|_| invalid())?;
        if after.st_dev != expected.st_dev || after.st_ino != expected.st_ino {
            return Err(invalid());
        }
        Ok(())
    }
}

#[cfg(unix)]
pub use unix::extract_verified_tool_archive_file;

#[cfg(not(unix))]
pub fn extract_verified_tool_archive_file(
    _archive: &std::fs::File,
    _sha256: &str,
    _version: &str,
    _destination: &std::path::Path,
) -> Result<(), crate::OrchestratorError> {
    Err(crate::internal::internal(
        "fd_tool_extract_unsupported_platform",
    ))
}

#[cfg(all(test, unix))]
mod promotion_tests {
    use super::unix::promote;
    use std::fs;

    #[test]
    fn replacing_the_staging_directory_cannot_publish_the_replacement()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let parent = fs::File::open(&root)?;
        fs::create_dir(root.join("stage"))?;
        let staged = fs::File::open(root.join("stage"))?;
        fs::rename(root.join("stage"), root.join("original"))?;
        fs::create_dir(root.join("stage"))?;
        fs::write(root.join("stage/attacker"), b"injected")?;
        assert!(promote(&parent, "stage", "result".as_ref(), &staged).is_err());
        assert!(!root.join("result").exists());
        Ok(())
    }
}
