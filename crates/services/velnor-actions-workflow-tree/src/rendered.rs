//! Rendered-tree envelopes: file/symlink records plus generated-tree paths.
//!
//! Pure data plus the fixed repository-relative output paths every tree
//! assembler shares; assembly and validation stay with the callers.

/// Generated actionlint config path inside the repository.
pub const ACTIONLINT_PATH: &str = ".github/actionlint.yaml";

/// One rendered file: repository-relative path plus bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFile {
    /// Repository-relative output path.
    pub path: String,
    /// Complete file bytes including the marker.
    pub bytes: String,
}

/// One rendered symbolic link: repository-relative link path plus target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSymlink {
    /// Repository-relative symlink path.
    pub path: String,
    /// Relative target path.
    pub target: String,
}

/// The generated files and symlinks, sorted by path: the base files
/// (actionlint config, CI workflow, AGENTS.md, and CLAUDE.md symlink) with release
/// disabled, plus the release family when release rendering is enabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedTree {
    /// Generated files in sorted path order.
    pub files: Vec<RenderedFile>,
    /// Generated symbolic links in sorted path order.
    pub symlinks: Vec<RenderedSymlink>,
}

impl RenderedTree {
    /// Fetch file bytes by repository-relative path.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.bytes.as_str())
    }

    /// Fetch symlink target by repository-relative path.
    #[must_use]
    pub fn get_symlink(&self, path: &str) -> Option<&str> {
        self.symlinks
            .iter()
            .find(|link| link.path == path)
            .map(|link| link.target.as_str())
    }

    /// Total count of all generated items (files plus symlinks).
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len() + self.symlinks.len()
    }

    /// Whether the tree contains no files and no symlinks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.symlinks.is_empty()
    }

    /// Sorted repository-relative paths of all files and symlinks.
    #[must_use]
    pub fn paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self
            .files
            .iter()
            .map(|f| f.path.clone())
            .chain(self.symlinks.iter().map(|s| s.path.clone()))
            .collect();
        paths.sort();
        paths
    }
}
