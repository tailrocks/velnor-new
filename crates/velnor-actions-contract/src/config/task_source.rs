//! Repository-local Mise source and working directory for an explicit task.

/// One repository-owned Mise config and its exact task working directory.
///
/// The working directory must be the config directory or one of its
/// descendants. Runtime validation also rejects symlinked path components and
/// hash-binds the selected files before task execution.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MiseTaskSource {
    /// Repository-relative `mise.toml` that declares the task.
    pub mise_config: String,
    /// Repository-relative directory from which the task is invoked.
    pub working_directory: String,
}

impl MiseTaskSource {
    /// `mise.lock` beside the declared config, if present.
    #[must_use]
    pub fn mise_lock_path(&self) -> String {
        self.sibling("mise.lock")
    }

    /// Idiomatic Rust toolchain file beside the declared config, if present.
    #[must_use]
    pub fn rust_toolchain_path(&self) -> String {
        self.sibling("rust-toolchain.toml")
    }

    /// Directory immediately above the source config directory.
    #[must_use]
    pub fn config_ceiling_directory(&self) -> String {
        let config_dir = self.config_directory();
        if config_dir == "." {
            "..".to_owned()
        } else {
            config_dir
                .rsplit_once('/')
                .map_or_else(|| ".".to_owned(), |(parent, _)| parent.to_owned())
        }
    }

    /// Validate canonical repository-relative paths and source containment.
    #[must_use]
    pub(crate) fn validate(&self) -> bool {
        valid_repository_path(&self.mise_config, false)
            && self.mise_config.ends_with("mise.toml")
            && valid_repository_path(&self.working_directory, true)
            && self.working_directory.len() <= 1024
            && self
                .mise_config
                .strip_suffix("mise.toml")
                .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('/'))
            && (self.config_directory() == "."
                || self.working_directory == self.config_directory()
                || self
                    .working_directory
                    .strip_prefix(self.config_directory())
                    .is_some_and(|suffix| suffix.starts_with('/')))
    }

    fn config_directory(&self) -> &str {
        self.mise_config
            .strip_suffix("/mise.toml")
            .filter(|directory| !directory.is_empty())
            .unwrap_or(".")
    }

    fn sibling(&self, name: &str) -> String {
        if self.config_directory() == "." {
            name.to_owned()
        } else {
            format!("{}/{name}", self.config_directory())
        }
    }
}

fn valid_repository_path(path: &str, root_allowed: bool) -> bool {
    if path == "." {
        return root_allowed;
    }
    !path.is_empty()
        && path.len() <= 1024
        && !path.starts_with('/')
        && !path.ends_with('/')
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

#[cfg(test)]
mod tests {
    use super::MiseTaskSource;

    #[test]
    fn source_paths_are_canonical_and_working_directory_stays_under_config_root() {
        let valid = MiseTaskSource {
            mise_config: "native/mise.toml".to_owned(),
            working_directory: "native/apple".to_owned(),
        };
        assert!(valid.validate());
        assert_eq!(valid.mise_lock_path(), "native/mise.lock");
        assert_eq!(valid.rust_toolchain_path(), "native/rust-toolchain.toml");
        assert_eq!(valid.config_ceiling_directory(), ".");

        let root = MiseTaskSource {
            mise_config: "mise.toml".to_owned(),
            working_directory: ".".to_owned(),
        };
        assert!(root.validate());
        assert_eq!(root.mise_lock_path(), "mise.lock");
        assert_eq!(root.config_ceiling_directory(), "..");

        for (mise_config, working_directory) in [
            ("/native/mise.toml", "native"),
            ("native/../mise.toml", "native"),
            ("native/mise.toml", "."),
            ("native/mise.toml", "native/../outside"),
            ("native/config.toml", "native"),
            ("native//mise.toml", "native"),
            ("native/mise.toml", "native/"),
        ] {
            let invalid = MiseTaskSource {
                mise_config: mise_config.to_owned(),
                working_directory: working_directory.to_owned(),
            };
            assert!(
                !invalid.validate(),
                "accepted {mise_config:?} from {working_directory:?}"
            );
        }
    }
}
