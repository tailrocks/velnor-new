// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn trust_mise_dir<'a>(command: &'a mut Command, dir: &Path) -> &'a mut Command {
    let inherited = std::env::var_os("MISE_TRUSTED_CONFIG_PATHS");
    command.env(
        "MISE_TRUSTED_CONFIG_PATHS",
        trusted_paths_with(inherited.as_deref(), dir),
    )
}

fn trusted_paths_with(inherited: Option<&OsStr>, dir: &Path) -> OsString {
    let mut trusted: Vec<PathBuf> = inherited
        .map(|value| std::env::split_paths(value).collect())
        .unwrap_or_default();
    trusted.push(dir.to_path_buf());
    std::env::join_paths(trusted)
        .unwrap_or_else(|error| unreachable!("MISE_TRUSTED_CONFIG_PATHS: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{trust_mise_dir, trusted_paths_with};
    use std::ffi::{OsStr, OsString};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn trusted_value(command: &Command) -> Option<String> {
        command
            .get_envs()
            .find(|(key, _)| *key == "MISE_TRUSTED_CONFIG_PATHS")
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().into_owned())
    }

    #[test]
    fn the_temp_dir_is_added_to_the_trusted_paths_of_the_child_only() {
        let mut command = Command::new("mise");
        trust_mise_dir(&mut command, Path::new("/tmp/velnor-project"));
        let value = trusted_value(&command).unwrap_or_else(|| unreachable!("no trust value"));
        let paths: Vec<_> = std::env::split_paths(&value).collect();
        assert_eq!(
            paths.last().map(PathBuf::as_path),
            Some(Path::new("/tmp/velnor-project"))
        );
    }

    #[test]
    fn inherited_trusted_paths_survive_and_the_temp_dir_is_appended_last() {
        let inherited = std::env::join_paths([Path::new("/work/existing"), Path::new("/work/other")])
            .unwrap_or_else(|error| unreachable!("{error}"));
        let value = trusted_paths_with(Some(inherited.as_os_str()), Path::new("/tmp/velnor-project"));
        let paths: Vec<PathBuf> = std::env::split_paths(&value).collect();
        assert_eq!(
            paths,
            [
                PathBuf::from("/work/existing"),
                PathBuf::from("/work/other"),
                PathBuf::from("/tmp/velnor-project"),
            ]
        );
    }

    #[test]
    fn without_an_inherited_value_only_the_temp_dir_is_trusted() {
        let value = trusted_paths_with(None::<&OsStr>, Path::new("/tmp/velnor-project"));
        assert_eq!(value, OsString::from("/tmp/velnor-project"));
    }
}
