// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

    pub(super) struct Tree {
        pub(super) root: File,
        pub(super) dirs: BTreeSet<String>,
        pub(super) created: Vec<(String, String, bool)>,
        pub(super) explicit: BTreeSet<String>,
        pub(super) aliases: BTreeMap<String, String>,
        pub(super) modes: Vec<(String, u32)>,
    }
    impl Tree {
        pub(super) fn new(root: File) -> Self {
            Self {
                root,
                dirs: BTreeSet::from([String::new()]),
                created: Vec::new(),
                explicit: BTreeSet::new(),
                aliases: BTreeMap::new(),
                modes: Vec::new(),
            }
        }
        pub(super) fn open_dir(&self, key: &str) -> Result<File, OrchestratorError> {
            let mut directory = self.root.try_clone().map_err(|_| invalid())?;
            if !key.is_empty() {
                for part in key.split('/') {
                    directory = File::from(
                        fs::openat(
                            &directory,
                            part,
                            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                            Mode::empty(),
                        )
                        .map_err(|_| invalid())?,
                    );
                }
            }
            Ok(directory)
        }
        pub(super) fn dir(&mut self, parts: &[&str]) -> Result<String, OrchestratorError> {
            let mut key = String::new();
            for part in parts {
                let parent = key.clone();
                if !key.is_empty() {
                    key.push('/');
                }
                key.push_str(part);
                if self.dirs.contains(&key) {
                    continue;
                }
                let fd = self.open_dir(&parent)?;
                fs::mkdirat(&fd, *part, Mode::from_raw_mode(0o700)).map_err(|_| invalid())?;
                self.created
                    .push((parent.clone(), (*part).to_owned(), true));
                let child = fs::openat(
                    &fd,
                    *part,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|_| invalid())?;
                let _child = File::from(child);
                self.dirs.insert(key.clone());
            }
            Ok(key)
        }
        pub(super) fn add<R: Read>(
            &mut self,
            name: &str,
            directory: bool,
            mode: u32,
            size: u64,
            reader: &mut R,
            deadline: Instant,
        ) -> Result<(), OrchestratorError> {
            let clean = name.strip_suffix('/').unwrap_or(name);
            let parts: Vec<_> = clean.split('/').collect();
            if clean.is_empty()
                || clean.len() > 4096
                || !clean.is_ascii()
                || clean.contains('\\')
                || parts.len() > 256
                || parts
                    .iter()
                    .any(|p| p.is_empty() || *p == "." || *p == "..")
                || mode & 0o7022 != 0
            {
                return Err(invalid());
            }
            let mut prefix = String::new();
            for part in &parts {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);
                let alias = prefix.to_ascii_lowercase();
                if self
                    .aliases
                    .get(&alias)
                    .is_some_and(|prior| prior != &prefix)
                {
                    return Err(invalid());
                }
                self.aliases.insert(alias, prefix.clone());
            }
            if !self.explicit.insert(clean.to_owned()) {
                return Err(invalid());
            }
            if directory {
                if size != 0 {
                    return Err(invalid());
                }
                let key = self.dir(&parts)?;
                self.modes.push((key, mode & 0o777));
                return Ok(());
            }
            if name.ends_with('/') || size > MAX_FILE {
                return Err(invalid());
            }
            let (leaf, parents) = parts.split_last().ok_or_else(invalid)?;
            let key = self.dir(parents)?;
            let parent = self.open_dir(&key)?;
            let fd = fs::openat(
                &parent,
                *leaf,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )
            .map_err(|_| invalid())?;
            self.created.push((key, (*leaf).to_owned(), false));
            let mut out = File::from(fd);
            let mut remaining = size;
            let mut buf = [0u8; 16384];
            while remaining > 0 {
                if Instant::now() >= deadline {
                    return Err(invalid());
                }
                let n = reader
                    .read(&mut buf[..usize::try_from(remaining.min(16384)).map_err(|_| invalid())?])
                    .map_err(|_| invalid())?;
                if n == 0 {
                    return Err(invalid());
                }
                out.write_all(&buf[..n]).map_err(|_| invalid())?;
                remaining -= n as u64;
            }
            fs::fchmod(
                &out,
                permissions(mode),
            )
            .map_err(|_| invalid())?;
            Ok(())
        }
        pub(super) fn finish_modes(&mut self, deadline: Instant) -> Result<(), OrchestratorError> {
            self.modes.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
            for (key, mode) in &self.modes {
                if Instant::now() >= deadline {
                    return Err(invalid());
                }
                fs::fchmod(
                    self.open_dir(key)?,
                    permissions(*mode),
                )
                .map_err(|_| invalid())?;
            }
            Ok(())
        }
        pub(super) fn cleanup(&mut self) -> Result<(), OrchestratorError> {
            let mut failed = false;
            let mut directories: Vec<_> = self.dirs.iter().collect();
            directories.sort_by_key(|path| path.len());
            for path in directories {
                match self.open_dir(path) {
                    Ok(fd) => failed |= fs::fchmod(fd, Mode::from_raw_mode(0o700)).is_err(),
                    Err(_) => failed = true,
                }
            }
            for (parent, name, directory) in self.created.iter().rev() {
                let fd = self.open_dir(parent)?;
                failed |= fs::unlinkat(
                    &fd,
                    name.as_str(),
                    if *directory {
                        AtFlags::REMOVEDIR
                    } else {
                        AtFlags::empty()
                    },
                )
                .is_err();
            }
            if failed {
                return Err(internal("fd_tool_extract_cleanup_failed"));
            }
            Ok(())
        }
    }
    fn permissions(mode: u32) -> Mode {
        [
            (0o400, Mode::RUSR), (0o200, Mode::WUSR), (0o100, Mode::XUSR),
            (0o040, Mode::RGRP), (0o020, Mode::WGRP), (0o010, Mode::XGRP),
            (0o004, Mode::ROTH), (0o002, Mode::WOTH), (0o001, Mode::XOTH),
        ].into_iter().fold(Mode::empty(), |result, (bit, flag)| {
            if mode & bit == 0 { result } else { result | flag }
        })
    }
    #[test]
    fn extracted_permission_bits_match_archive_modes() {
        for mode in 0..=0o777 {
            assert_eq!(u64::from(permissions(mode).bits()), u64::from(mode));
        }
    }
