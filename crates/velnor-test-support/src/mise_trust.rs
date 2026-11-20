// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

/// Makes mise trust the temporary project `dir` for this child only, so a test that writes a
/// `mise.toml` into a temp folder and runs mise (directly, or through a `velnor-ci` or cargo
/// command wrapper) behaves the same on a developer machine as on CI. `dir` is appended to the
/// inherited `MISE_TRUSTED_CONFIG_PATHS` with the platform path separator; the parent's value is
/// not changed.
///
/// # Panics
///
/// When an inherited trusted path cannot be split or joined (it contains the separator).
pub fn trust_mise_dir<'a>(command: &'a mut Command, dir: &Path) -> &'a mut Command {
    let mut trusted: Vec<PathBuf> = std::env::var_os("MISE_TRUSTED_CONFIG_PATHS")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    trusted.push(dir.to_path_buf());
    let joined = std::env::join_paths(trusted)
        .unwrap_or_else(|error| unreachable!("MISE_TRUSTED_CONFIG_PATHS: {error}"));
    command.env("MISE_TRUSTED_CONFIG_PATHS", joined)
}

#[cfg(test)]
mod tests {
    use super::trust_mise_dir;
    use std::path::Path;
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
            paths.last().map(|p| p.as_path()),
            Some(Path::new("/tmp/velnor-project"))
        );
    }
}
