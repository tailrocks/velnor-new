// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::os::unix::fs::{DirBuilderExt, symlink};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::CiError;

const GIT: &str = "git";
const REGISTRY: &str = "registry";
const DOWNLOADS: [&str; 2] = ["cache", "src"];
const ATTEMPTS: u32 = 16;

pub(super) struct Scratch {
    path: PathBuf,
    cache: PathBuf,
}

impl Scratch {
    pub(super) fn create(cache: &Path, temp: &Path, trusted_cache: bool) -> Result<Self, CiError> {
        let scratch = Self {
            path: fresh_directory(temp)?,
            cache: cache.to_path_buf(),
        };
        scratch.prepare(trusted_cache).map_err(|e| {
            CiError::new(format!(
                "cannot prepare {} for the fetch: {e}",
                cache.display()
            ))
        })?;
        Ok(scratch)
    }

    fn prepare(&self, trusted_cache: bool) -> std::io::Result<()> {
        if trusted_cache {
            self.share(REGISTRY)?;
            return self.share(GIT);
        }
        let registry = self.path.join(REGISTRY);
        std::fs::create_dir(&registry)?;
        std::fs::create_dir(registry.join("index"))?;
        for name in DOWNLOADS {
            let target = self.cache.join(REGISTRY).join(name);
            std::fs::create_dir_all(&target)?;
            symlink(&target, registry.join(name))?;
        }
        std::fs::create_dir(self.path.join(GIT))
    }

    fn share(&self, name: &str) -> std::io::Result<()> {
        let target = self.cache.join(name);
        std::fs::create_dir_all(&target)?;
        symlink(&target, self.path.join(name))
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn keep_git(&self) -> Result<(), CiError> {
        let fetched = self.path.join(GIT);
        if std::fs::symlink_metadata(&fetched).is_ok_and(|meta| meta.is_dir()) {
            super::fetch_git::merge(&fetched, &self.cache.join(GIT))?;
        }
        Ok(())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.path));
    }
}

pub(super) fn temp_parent(home: &Path) -> PathBuf {
    let home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    std::env::var_os("RUNNER_TEMP")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute() && dir.is_dir())
        .filter(|dir| !std::fs::canonicalize(dir).is_ok_and(|dir| dir.starts_with(&home)))
        .unwrap_or_else(std::env::temp_dir)
}

pub(super) fn cache_home(home: &Path, job: Option<&OsStr>) -> PathBuf {
    let default = home.join(".cargo");
    let Some(job) = job.filter(|job| !job.is_empty()).map(PathBuf::from) else {
        return default;
    };
    let plain = job.is_absolute() && !job.components().any(|c| c == Component::ParentDir);
    let canonical = std::fs::canonicalize(&default).unwrap_or_else(|_| default.clone());
    if plain && (job.starts_with(&default) || job.starts_with(&canonical)) {
        job
    } else {
        default
    }
}

fn fresh_directory(temp: &Path) -> Result<PathBuf, CiError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    for attempt in 0..ATTEMPTS {
        let path = temp.join(format!(
            "velnor-fetch-cargo-home-{}-{nanos}-{attempt}",
            std::process::id()
        ));
        match std::fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => {
                return std::fs::canonicalize(&path)
                    .map_err(|e| CiError::new(format!("cannot resolve {}: {e}", path.display())));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                return Err(CiError::new(format!(
                    "cannot create the fetch's CARGO_HOME under {}: {e}",
                    temp.display()
                )));
            }
        }
    }
    Err(CiError::new(format!(
        "cannot create a fresh CARGO_HOME under {}",
        temp.display()
    )))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("velnor-home-{name}-{}", std::process::id()));
        drop(fs::remove_dir_all(&root));
        fs::create_dir_all(root.join("home/.cargo")).unwrap_or_else(|e| unreachable!("{e}"));
        fs::create_dir_all(root.join("tmp")).unwrap_or_else(|e| unreachable!("{e}"));
        fs::canonicalize(&root).unwrap_or_else(|e| unreachable!("{e}"))
    }

    fn entries(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .unwrap_or_else(|e| unreachable!("{e}"))
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_cargo_config_planted_in_the_runner_home_never_reaches_the_fetch() {
        let root = tree("planted");
        let cargo = root.join("home/.cargo");
        for name in ["config.toml", "config", "credentials.toml"] {
            fs::write(
                cargo.join(name),
                "[source.crates-io]\nreplace-with = \"evil\"\n[source.evil]\nregistry = \"sparse+https://evil.invalid/\"\n[target.x86_64-unknown-linux-gnu]\nrunner = \"/tmp/evil\"\n",
            )
            .unwrap_or_else(|e| unreachable!("{e}"));
        }
        let scratch = Scratch::create(&root.join("home/.cargo"), &root.join("tmp"), true)
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(entries(scratch.path()), ["git", "registry"]);
        assert_ne!(scratch.path(), cargo);
        assert!(!scratch.path().starts_with(&cargo));
    }

    #[test]
    fn the_job_cargo_home_inside_the_runner_home_receives_the_fetched_sources() {
        let root = tree("job");
        let job = root.join("home/.cargo/nested");
        let scratch = Scratch::create(
            &cache_home(&root.join("home"), Some(job.as_os_str())),
            &root.join("tmp"),
            true,
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(scratch.path().join("registry/crate"), "")
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(job.join("registry/crate").is_file());
    }

    #[test]
    fn a_job_cargo_home_outside_the_runner_home_or_climbing_out_of_it_is_not_used() {
        let root = tree("outside");
        let home = root.join("home");
        for job in [
            root.join("elsewhere/cargo"),
            home.join(".cargo/../.ssh"),
            PathBuf::from("relative/cargo"),
            PathBuf::from("/opt/rust/cargo"),
        ] {
            assert_eq!(
                cache_home(&home, Some(job.as_os_str())),
                home.join(".cargo")
            );
        }
        assert_eq!(cache_home(&home, Some(OsStr::new(""))), home.join(".cargo"));
    }

    #[test]
    fn the_scratch_home_is_private_and_removed_without_touching_the_shared_sources() {
        use std::os::unix::fs::MetadataExt;
        let root = tree("drop");
        let scratch = Scratch::create(&root.join("home/.cargo"), &root.join("tmp"), true)
            .unwrap_or_else(|e| unreachable!("{e}"));
        let path = scratch.path().to_path_buf();
        let mode = fs::metadata(&path)
            .unwrap_or_else(|e| unreachable!("{e}"))
            .mode();
        assert_eq!(mode & 0o077, 0);
        fs::write(root.join("home/.cargo/registry/kept"), "")
            .unwrap_or_else(|e| unreachable!("{e}"));
        drop(scratch);
        assert!(!path.exists());
        assert!(root.join("home/.cargo/registry/kept").is_file());
    }

    fn files(directory: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut pending = vec![directory.to_path_buf()];
        while let Some(next) = pending.pop() {
            let Ok(read) = fs::read_dir(&next) else {
                continue;
            };
            for entry in read.filter_map(Result::ok) {
                found.push(entry.path());
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    pending.push(entry.path());
                }
            }
        }
        found
    }

    #[test]
    fn the_token_fetch_never_sees_a_cached_registry_index_repository() {
        let plain = "[core]\n\trepositoryformatversion = 0\n\tbare = false\n";
        let planted = "[http]\n\tproxy = http://attacker.invalid:8080\n";
        let layouts: [(&str, &str, &str); 3] = [
            ("commondir", ".git/commondir", "../../attacker"),
            ("nested-dir", ".git/.git/config", planted),
            ("nested-gitdir", ".git/.git", "gitdir: ../../attacker\n"),
        ];
        for (label, path, text) in layouts {
            let root = tree(&format!("index-{label}"));
            let cache = root.join("home/.cargo");
            let index = cache.join("registry/index/github.com-1ecc6299db9ec823");
            for (file, body) in [
                (index.join(".git/config"), plain),
                (cache.join("attacker/config"), planted),
                (index.join(path), text),
            ] {
                fs::create_dir_all(file.parent().unwrap_or_else(|| unreachable!()))
                    .and_then(|()| fs::write(&file, body))
                    .unwrap_or_else(|e| unreachable!("{label}: {e}"));
            }
            let scratch = Scratch::create(&cache, &root.join("tmp"), false)
                .unwrap_or_else(|e| unreachable!("{e}"));
            let fresh = scratch.path().join("registry/index");
            let meta =
                fs::symlink_metadata(&fresh).unwrap_or_else(|e| unreachable!("{label}: {e}"));
            assert!(meta.is_dir() && !meta.file_type().is_symlink(), "{label}");
            assert_eq!(files(&fresh), Vec::<PathBuf>::new(), "{label}");
        }
    }

    #[test]
    fn the_token_fetch_starts_with_an_empty_git_folder_whatever_the_cache_holds() {
        let root = tree("git-db");
        let cache = root.join("home/.cargo");
        let database = cache.join("git/db/dep-1a2b3c");
        fs::create_dir_all(&database).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(database.join("commondir"), "../../../attacker")
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(database.join("HEAD"), "ref: refs/heads/main\n")
            .unwrap_or_else(|e| unreachable!("{e}"));
        let scratch = Scratch::create(&cache, &root.join("tmp"), false)
            .unwrap_or_else(|e| unreachable!("{e}"));
        let git = scratch.path().join("git");
        let meta = fs::symlink_metadata(&git).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(meta.is_dir() && !meta.file_type().is_symlink());
        assert_eq!(files(&git), Vec::<PathBuf>::new());
    }

    #[test]
    fn the_token_fetch_still_shares_the_downloaded_crates_with_the_cache() {
        let root = tree("crates");
        let cache = root.join("home/.cargo");
        let scratch = Scratch::create(&cache, &root.join("tmp"), false)
            .unwrap_or_else(|e| unreachable!("{e}"));
        for part in ["cache", "src"] {
            fs::write(scratch.path().join("registry").join(part).join("serde"), "")
                .unwrap_or_else(|e| unreachable!("{part}: {e}"));
            assert!(cache.join("registry").join(part).join("serde").is_file());
        }
    }

    #[test]
    fn two_scratch_homes_never_share_a_directory() {
        let root = tree("pair");
        let first = Scratch::create(&root.join("home/.cargo"), &root.join("tmp"), true)
            .unwrap_or_else(|e| unreachable!("{e}"));
        let second = Scratch::create(&root.join("home/.cargo"), &root.join("tmp"), true)
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_ne!(first.path(), second.path());
    }
}
