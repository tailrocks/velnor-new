// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::error::CiError;

use super::fetch::{Inherited, Trusted};

const BASE: &str = "/opt/rust";
const GROUP_AND_OTHER_WRITE: u32 = 0o022;

#[derive(Clone)]
pub(super) struct Fixed {
    base: PathBuf,
    owner: u32,
    ceiling: PathBuf,
}

pub(super) struct Located {
    pub(super) cargo: PathBuf,
    seal: Fixed,
    cargo_home: PathBuf,
    rustup_home: PathBuf,
}

impl Fixed {
    pub(super) fn system() -> Self {
        Self {
            base: BASE.into(),
            owner: 0,
            ceiling: "/".into(),
        }
    }

    pub(super) fn find(&self) -> Option<Located> {
        let cargo_home = self.base.join("cargo");
        let candidate = cargo_home.join("bin/cargo");
        let resolved = std::fs::canonicalize(&candidate).ok()?;
        let confined = std::fs::canonicalize(&self.base).ok()? == self.base
            && resolved.starts_with(&self.base);
        (confined
            && resolved.is_file()
            && !super::fetch_real_cargo::is_mise_front(&candidate, &resolved)
            && self.sealed(&candidate)
            && self.sealed(&resolved))
        .then(|| Located {
            cargo: candidate,
            seal: self.clone(),
            cargo_home,
            rustup_home: self.base.join("rustup"),
        })
    }

    fn sealed(&self, path: &Path) -> bool {
        path.ancestors()
            .take_while(|ancestor| ancestor.starts_with(&self.ceiling))
            .all(|ancestor| {
                std::fs::symlink_metadata(ancestor).is_ok_and(|meta| {
                    meta.uid() == self.owner
                        && (meta.file_type().is_symlink()
                            || meta.mode() & GROUP_AND_OTHER_WRITE == 0)
                })
            })
    }
}

impl Located {
    pub(super) fn seals(&self, path: &Path) -> bool {
        self.seal.sealed(path)
    }

    pub(super) fn roots(&self) -> [PathBuf; 2] {
        [self.cargo_home.clone(), self.rustup_home.clone()]
    }

    pub(super) fn adopt(&self, scratch: &Path) -> Inherited {
        Inherited {
            cargo_home: Some(OsString::from(scratch)),
            rustup_home: Some(OsString::from(&self.rustup_home)),
        }
    }
}

pub(super) fn cargo(
    fixed: Option<&Located>,
    trusted: &Trusted,
    home: &Path,
    search: &[PathBuf],
) -> Result<PathBuf, CiError> {
    if let Some(fixed) = fixed {
        return Ok(fixed.cargo.clone());
    }
    let mut search = search.to_vec();
    search.push(home.join(".cargo/bin"));
    trusted.program("cargo", &search, false)
}

pub(super) fn account_home(chosen: Option<&Path>) -> Result<PathBuf, CiError> {
    if let Some(chosen) = chosen {
        return Ok(chosen.to_path_buf());
    }
    nix::unistd::User::from_uid(nix::unistd::Uid::current())
        .ok()
        .flatten()
        .map(|user| user.dir)
        .filter(|dir| dir.is_absolute())
        .ok_or_else(|| CiError::new("cannot read the home directory of the runner account"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::super::fetch_home::Scratch;
    use super::*;

    fn me() -> u32 {
        nix::unistd::Uid::current().as_raw()
    }

    fn script(path: &Path) {
        fs::create_dir_all(path.parent().unwrap_or_else(|| unreachable!()))
            .and_then(|()| fs::write(path, "#!/bin/sh\n"))
            .and_then(|()| fs::set_permissions(path, fs::Permissions::from_mode(0o755)))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }

    fn sealed_tree(name: &str) -> (PathBuf, Fixed) {
        let root = std::env::temp_dir().join(format!("velnor-fixed-{name}-{}", std::process::id()));
        drop(fs::remove_dir_all(&root));
        let root = {
            fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
            fs::canonicalize(&root).unwrap_or_else(|e| unreachable!("{e}"))
        };
        let base = root.join("opt/rust");
        script(&base.join("cargo/bin/cargo"));
        fs::create_dir_all(base.join("rustup")).unwrap_or_else(|e| unreachable!("{e}"));
        for dir in [&root, &root.join("opt"), &base] {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o755))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        let fixed = Fixed {
            base,
            owner: me(),
            ceiling: root.clone(),
        };
        (root, fixed)
    }

    #[test]
    fn a_sealed_toolchain_is_found_with_its_rustup_home_adopted() {
        let (_, fixed) = sealed_tree("sealed");
        let found = fixed.find().unwrap_or_else(|| unreachable!("not found"));
        assert_eq!(found.cargo, fixed.base.join("cargo/bin/cargo"));
        let adopted = found.adopt(Path::new("/scratch"));
        assert_eq!(adopted.rustup_home, Some(fixed.base.join("rustup").into()));
        assert_eq!(adopted.cargo_home, Some("/scratch".into()));
    }

    #[test]
    fn without_a_job_cargo_home_the_sealed_toolchain_fetches_into_a_writable_home() {
        let (root, fixed) = sealed_tree("writable");
        let found = fixed.find().unwrap_or_else(|| unreachable!("not found"));
        let home = root.join("home");
        let temp = root.join("tmp");
        fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
        let scratch = Scratch::create(&home.join(".cargo"), &temp, true)
            .unwrap_or_else(|e| unreachable!("{e}"));
        let adopted = found.adopt(scratch.path());
        let chosen = PathBuf::from(adopted.cargo_home.unwrap_or_default());
        assert!(
            !chosen.starts_with(root.join("opt")),
            "{}",
            chosen.display()
        );
        fs::write(chosen.join(".package-cache"), "").unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(chosen.join("registry/index"), "").unwrap_or_else(|e| unreachable!("{e}"));
        assert!(home.join(".cargo/registry/index").is_file());
    }

    fn beside_cargo(fixed: &Fixed, found: &Located) -> Option<PathBuf> {
        let trusted = Trusted::new(&fixed.base, &found.roots());
        let cargo = fs::canonicalize(&found.cargo).unwrap_or_else(|e| unreachable!("{e}"));
        super::super::fetch_toolchain::beside(&found.cargo, &cargo, &trusted, Some(found))
    }

    #[test]
    fn a_rustup_next_to_the_sealed_cargo_is_used_only_when_it_is_sealed_too() {
        let (_, fixed) = sealed_tree("rustup");
        let rustup = fixed.base.join("cargo/bin/rustup");
        script(&rustup);
        let found = fixed.find().unwrap_or_else(|| unreachable!("not found"));
        let sealed = fs::canonicalize(&rustup).ok();
        assert_eq!(beside_cargo(&fixed, &found), sealed);
        for mode in [0o775, 0o757] {
            fs::set_permissions(&rustup, fs::Permissions::from_mode(mode))
                .unwrap_or_else(|e| unreachable!("{e}"));
            assert_eq!(beside_cargo(&fixed, &found), None, "{mode:o}");
        }
    }

    #[test]
    fn a_rustup_link_from_the_sealed_toolchain_to_a_writable_place_is_not_used() {
        let (root, fixed) = sealed_tree("rustup-link");
        let elsewhere = root.join("home/.cargo/bin/rustup");
        script(&elsewhere);
        fs::set_permissions(
            root.join("home/.cargo/bin"),
            fs::Permissions::from_mode(0o775),
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        symlink(&elsewhere, fixed.base.join("cargo/bin/rustup"))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let found = fixed.find().unwrap_or_else(|| unreachable!("not found"));
        let trusted = Trusted::new(&root.join("home"), &found.roots());
        let cargo = fs::canonicalize(&found.cargo).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(
            super::super::fetch_toolchain::beside(&found.cargo, &cargo, &trusted, Some(&found)),
            None
        );
    }

    #[test]
    fn a_toolchain_with_a_group_writable_ancestor_is_refused() {
        let (_, fixed) = sealed_tree("group");
        fs::set_permissions(fixed.base.join("cargo"), fs::Permissions::from_mode(0o775))
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(fixed.find().is_none());
    }

    #[test]
    fn a_world_writable_cargo_file_is_refused() {
        let (_, fixed) = sealed_tree("world");
        let cargo = fixed.base.join("cargo/bin/cargo");
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o757))
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(fixed.find().is_none());
    }

    #[test]
    fn a_toolchain_owned_by_someone_else_is_refused() {
        let (_, mut fixed) = sealed_tree("owner");
        fixed.owner = me() + 1;
        assert!(fixed.find().is_none());
    }

    #[test]
    fn a_cargo_link_that_leaves_the_toolchain_is_refused() {
        let (root, fixed) = sealed_tree("escape");
        script(&root.join("elsewhere/cargo"));
        let cargo = fixed.base.join("cargo/bin/cargo");
        fs::remove_file(&cargo).unwrap_or_else(|e| unreachable!("{e}"));
        symlink(root.join("elsewhere/cargo"), &cargo).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(fixed.find().is_none());
    }

    #[test]
    fn the_sealed_toolchain_wins_over_the_home_and_the_home_is_tried_when_the_path_has_no_cargo() {
        let (root, fixed) = sealed_tree("select");
        let home = root.join("home");
        script(&home.join(".cargo/bin/cargo"));
        let found = fixed.find();
        let trusted = Trusted::new(
            &home,
            &found.iter().flat_map(Located::roots).collect::<Vec<_>>(),
        );
        let chosen =
            cargo(found.as_ref(), &trusted, &home, &[]).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(chosen, fixed.base.join("cargo/bin/cargo"));
        let chosen = cargo(None, &trusted, &home, &[]).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(
            chosen,
            fs::canonicalize(&home)
                .unwrap_or_default()
                .join(".cargo/bin/cargo")
        );
    }
}
