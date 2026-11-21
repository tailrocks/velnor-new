// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::{self, Path, PathBuf};
use std::process::Command;

use nix::errno::Errno;
use nix::unistd::{AccessFlags, access};

const DEFAULT_PATH: &str = "/usr/bin:/bin";

const NATIVE_MAGIC: [&[u8]; 6] = [
    b"\x7fELF",
    b"\xfe\xed\xfa\xce",
    b"\xfe\xed\xfa\xcf",
    b"\xce\xfa\xed\xfe",
    b"\xcf\xfa\xed\xfe",
    b"\xca\xfe\xba\xbe",
];

fn search_path(build: &Command) -> OsString {
    build
        .get_envs()
        .find(|(name, _)| *name == "PATH")
        .map_or_else(
            || std::env::var_os("PATH"),
            |(_, value)| value.map(OsStr::to_owned),
        )
        .unwrap_or_else(|| DEFAULT_PATH.into())
}

fn usable(candidate: &Path, metadata: &fs::Metadata) -> io::Result<PathBuf> {
    if !metadata.is_file() {
        return Err(Errno::EACCES.into());
    }
    access(candidate, AccessFlags::X_OK).map_err(io::Error::from)?;
    path::absolute(candidate)
}

fn executable_format(path: PathBuf) -> io::Result<PathBuf> {
    let mut head = Vec::new();
    let read = fs::File::open(&path).and_then(|file| file.take(4).read_to_end(&mut head));
    let known = head.starts_with(b"#!") || NATIVE_MAGIC.iter().any(|magic| head.starts_with(magic));
    if read.is_err() || known {
        Ok(path)
    } else {
        Err(Errno::ENOEXEC.into())
    }
}

pub(super) fn resolve(build: &Command) -> io::Result<PathBuf> {
    let program = Path::new(build.get_program());
    if program.as_os_str().is_empty() {
        return Err(Errno::ENOENT.into());
    }
    let base = match build.get_current_dir() {
        Some(directory) => path::absolute(directory)?,
        None => std::env::current_dir()?,
    };
    if program.as_os_str().as_bytes().contains(&b'/') {
        let candidate = base.join(program);
        return usable(&candidate, &fs::metadata(&candidate)?).and_then(executable_format);
    }
    let mut denied = false;
    for directory in std::env::split_paths(&search_path(build)) {
        let candidate = base.join(directory).join(program);
        let Ok(metadata) = fs::metadata(&candidate) else {
            continue;
        };
        match usable(&candidate, &metadata) {
            Ok(found) => return executable_format(found),
            Err(_) => denied = true,
        }
    }
    Err(if denied { Errno::EACCES } else { Errno::ENOENT }.into())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory = fs::canonicalize(std::env::temp_dir())
            .unwrap_or_else(|e| unreachable!("{e}"))
            .join(format!("velnor-program-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
        directory
    }

    fn file(directory: &Path, name: &str, mode: u32) -> PathBuf {
        let path = directory.join(name);
        fs::write(&path, "#!/bin/sh\n").unwrap_or_else(|e| unreachable!("{e}"));
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))
            .unwrap_or_else(|e| unreachable!("{e}"));
        path
    }

    fn kind(program: &str, directory: &Path, path: &Path) -> io::ErrorKind {
        let mut build = Command::new(program);
        build.current_dir(directory).env("PATH", path);
        resolve(&build)
            .err()
            .unwrap_or_else(|| unreachable!("{program:?} must not resolve"))
            .kind()
    }

    #[test]
    fn a_missing_program_is_not_found_by_name_by_path_and_when_empty() {
        let directory = scratch("missing");
        for program in ["absent", "./absent", "", "sub/absent"] {
            assert_eq!(
                kind(program, &directory, &directory),
                io::ErrorKind::NotFound,
                "{program:?}"
            );
        }
        fs::remove_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn a_file_without_execute_permission_or_a_directory_is_permission_denied() {
        let directory = scratch("denied");
        file(&directory, "plain", 0o644);
        fs::create_dir(directory.join("folder")).unwrap_or_else(|e| unreachable!("{e}"));
        for program in ["plain", "./plain", "./folder", "folder"] {
            assert_eq!(
                kind(program, &directory, &directory),
                io::ErrorKind::PermissionDenied,
                "{program:?}"
            );
        }
        fs::remove_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    }

    fn found(program: &str, directory: &Path, path: &OsStr) -> io::Result<PathBuf> {
        let mut build = Command::new(program);
        build.current_dir(directory).env("PATH", path);
        resolve(&build)
    }

    fn joined(directories: &[PathBuf]) -> OsString {
        std::env::join_paths(directories).unwrap_or_else(|e| unreachable!("{e}"))
    }

    fn directories(directory: &Path, names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|name| {
                let path = directory.join(name);
                fs::create_dir_all(&path).unwrap_or_else(|e| unreachable!("{e}"));
                path
            })
            .collect()
    }

    #[test]
    fn the_first_executable_on_path_wins() {
        let directory = scratch("order");
        let dirs = directories(&directory, &["first", "second"]);
        let expected = file(&dirs[0], "tool", 0o755);
        file(&dirs[1], "tool", 0o755);
        assert_eq!(
            found("tool", &directory, &joined(&dirs)).ok(),
            Some(expected)
        );
        fs::remove_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn a_non_executable_candidate_is_skipped_for_a_later_executable() {
        let directory = scratch("skipped");
        let dirs = directories(&directory, &["first", "second"]);
        file(&dirs[0], "tool", 0o644);
        let expected = file(&dirs[1], "tool", 0o755);
        assert_eq!(
            found("tool", &directory, &joined(&dirs)).ok(),
            Some(expected)
        );
        fs::remove_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn an_empty_path_component_means_the_build_directory() {
        let directory = scratch("empty");
        let dirs = directories(&directory, &["elsewhere"]);
        let expected = file(&directory, "tool", 0o755);
        let path = OsString::from(format!(":{}", dirs[0].display()));
        assert_eq!(found("tool", &directory, &path).ok(), Some(expected));
        fs::remove_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn a_relative_path_component_is_relative_to_the_build_directory() {
        let directory = scratch("relative");
        let dirs = directories(&directory, &["sub"]);
        let expected = file(&dirs[0], "tool", 0o755);
        assert_eq!(
            found("tool", &directory, OsStr::new("sub")).ok(),
            Some(expected)
        );
        fs::remove_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn with_path_removed_the_search_uses_the_platform_default() {
        let mut build = Command::new("true");
        build.env_remove("PATH");
        assert!(resolve(&build).is_ok());
        #[cfg(target_os = "macos")]
        {
            let mut sbin = Command::new("ifconfig");
            sbin.env_remove("PATH");
            assert_eq!(
                resolve(&sbin).err().map(|e| e.kind()),
                Some(io::ErrorKind::NotFound)
            );
        }
    }

    #[test]
    fn an_executable_resolves_to_an_absolute_path_that_never_starts_with_a_dash() {
        let directory = scratch("found");
        let dashed = file(&directory, "-tool", 0o755);
        let named = file(&directory, "named", 0o755);
        let mut relative = Command::new("./-tool");
        relative.current_dir(&directory).env("PATH", "/nonexistent");
        assert_eq!(resolve(&relative).ok(), Some(dashed));
        let mut searched = Command::new("named");
        searched.current_dir(&directory).env("PATH", &directory);
        assert_eq!(resolve(&searched).ok(), Some(named));
        fs::remove_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    }
}
