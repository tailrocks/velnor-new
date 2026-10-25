#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MiseTaskSource {
    pub mise_config: String,

    pub working_directory: String,
}

impl MiseTaskSource {
    #[must_use]
    pub fn mise_lock_path(&self) -> String {
        self.sibling("mise.lock")
    }

    #[must_use]
    pub fn rust_toolchain_path(&self) -> String {
        self.sibling("rust-toolchain.toml")
    }

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

impl Default for MiseTaskSource {
    fn default() -> Self {
        Self {
            mise_config: "mise.toml".to_owned(),
            working_directory: ".".to_owned(),
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
