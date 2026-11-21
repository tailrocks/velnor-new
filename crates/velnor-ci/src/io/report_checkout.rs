// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use velnor_model::{RepoPath, RepoView};
use velnor_rust::{METADATA_PATH, recorded_metadata, wanted_files};

pub(super) struct ReportCheckout {
    pub checkout: velnor_gen::io::Checkout,
    pub symlink_locks: BTreeSet<RepoPath>,
}

fn ci(error: impl std::fmt::Display) -> CiError {
    CiError::new(error.to_string())
}

fn gitlinks(top: &Path) -> Result<BTreeSet<RepoPath>, CiError> {
    let listed = super::inputs::git(top, &["ls-files", "-s", "-z"], None)?;
    listed
        .split(|byte| *byte == 0)
        .filter(|entry| entry.starts_with(b"160000 "))
        .map(|entry| {
            let tab = entry
                .iter()
                .position(|byte| *byte == b'\t')
                .ok_or_else(|| ci("git ls-files printed an entry without a path"))?;
            let text = std::str::from_utf8(&entry[tab + 1..]).map_err(ci)?;
            RepoPath::new(text).map_err(ci)
        })
        .collect()
}

pub(super) fn read(directory: &Path) -> Result<ReportCheckout, CiError> {
    let tracked = super::inputs::tracked_paths(directory)?;
    let tracked_paths = tracked
        .paths
        .iter()
        .filter_map(|path| path.to_str())
        .map(RepoPath::new)
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(ci)?;
    let symlinks: BTreeSet<_> = tracked_paths
        .iter()
        .filter(|path| {
            fs::symlink_metadata(tracked.top.join(path.as_str()))
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
        })
        .cloned()
        .collect();
    let symlink_locks: BTreeSet<_> = symlinks
        .iter()
        .filter(|path| {
            Path::new(path.as_str())
                .file_name()
                .is_some_and(|name| name == "Cargo.lock")
        })
        .cloned()
        .collect();
    let wanted: Vec<_> = wanted_files(&tracked_paths)
        .into_iter()
        .filter(|path| {
            !symlink_locks.contains(path)
                && (!symlinks.contains(path)
                    || !std::path::Path::new(path.as_str())
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs")))
        })
        .collect();
    let mut view = RepoView {
        repository: velnor_gen::io::repository_name(&tracked.top),
        files: velnor_gen::io::read_files(&tracked.top, &wanted).map_err(ci)?,
        tracked_paths,
        symlinks,
        gitlinks: gitlinks(&tracked.top)?,
        rust_metadata: BTreeMap::new(),
    };
    velnor_gen::io::add_manifests(&mut view, |paths| {
        velnor_gen::io::read_files(&tracked.top, paths)
    })
    .map_err(ci)?;
    let mut recorded = velnor_gen::io::read_files(&tracked.top, &[METADATA_PATH]).map_err(ci)?;
    view.rust_metadata =
        recorded_metadata(&view, recorded.remove(&METADATA_PATH).as_deref()).map_err(ci)?;
    Ok(ReportCheckout {
        checkout: velnor_gen::io::Checkout {
            root: tracked.top,
            view,
        },
        symlink_locks,
    })
}

#[cfg(test)]
mod tests {
    use super::{gitlinks, read};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn git(directory: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(args)
            .status()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn a_submodule_entry_is_a_gitlink_and_a_file_is_not() {
        let directory: PathBuf =
            std::env::temp_dir().join(format!("velnor-ci-gitlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
        git(&directory, &["init", "--quiet", "--object-format=sha1"]);
        git(
            &directory,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                "160000,1111111111111111111111111111111111111111,vendor/lib",
            ],
        );
        git(
            &directory,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                "100644,e69de29bb2d1d6434b8b29ae775ad8c2e48c5391,Cargo.toml",
            ],
        );
        let found = gitlinks(&directory);
        let _ = std::fs::remove_dir_all(&directory);
        let found: Vec<String> = found
            .unwrap_or_else(|e| unreachable!("{e}"))
            .iter()
            .map(|path| path.as_str().to_owned())
            .collect();
        assert_eq!(found, ["vendor/lib"]);
    }

    #[test]
    fn a_configurations_file_inside_a_submodule_is_refused_by_the_runtime_check() {
        let directory: PathBuf =
            std::env::temp_dir().join(format!("velnor-ci-submodule-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let submodule = directory.join("vendor/lib");
        std::fs::create_dir_all(&submodule).unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(submodule.join("configurations.json"), "{}\n")
            .unwrap_or_else(|e| unreachable!("{e}"));
        git(&submodule, &["init", "--quiet", "--object-format=sha1"]);
        git(&submodule, &["add", "configurations.json"]);
        git(
            &submodule,
            &[
                "-c",
                "user.name=velnor-test",
                "-c",
                "user.email=velnor-test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "init",
            ],
        );
        std::fs::write(
            directory.join("Cargo.toml"),
            "[package]\nname = 'app'\nversion = '0.1.0'\nedition = '2024'\n",
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::create_dir_all(directory.join("src")).unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(directory.join("src/lib.rs"), "\n").unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(
            directory.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\n",
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::create_dir_all(directory.join(".velnor")).unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(
            directory.join(".velnor/rust-ci.toml"),
            "[[check_configurations]]\nroot = '.'\nconfigurations = 'vendor/lib/configurations.json'\n",
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(
            directory.join(".gitmodules"),
            "[submodule \"vendor/lib\"]\n\tpath = vendor/lib\n\turl = ./vendor/lib\n",
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        git(&directory, &["init", "--quiet", "--object-format=sha1"]);
        git(
            &directory,
            &[
                "add",
                ".gitmodules",
                "Cargo.toml",
                "Cargo.lock",
                "src/lib.rs",
                ".velnor/rust-ci.toml",
            ],
        );
        git(&directory, &["add", "vendor/lib"]);
        git(&directory, &["submodule", "init", "--quiet"]);
        velnor_gen::io::write_metadata(&directory).unwrap_or_else(|e| unreachable!("{e}"));
        let outcome = read(&directory)
            .map_err(|e| e.to_string())
            .and_then(|report| {
                velnor_rust::detect(&report.checkout.view).map_err(|e| e.to_string())
            });
        let _ = std::fs::remove_dir_all(&directory);
        let error = outcome
            .err()
            .unwrap_or_else(|| unreachable!("the submodule path was accepted"));
        assert!(error.contains("submodule"), "{error}");
    }
}
