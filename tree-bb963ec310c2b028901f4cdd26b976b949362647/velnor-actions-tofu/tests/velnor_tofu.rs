//! Integration test entry point; cases live in the sibling files.
#[path = "impl_tofu_argv.rs"]
mod impl_tofu_argv;
#[path = "impl_tofu_closure.rs"]
mod impl_tofu_closure;
#[path = "impl_tofu_closure_b.rs"]
mod impl_tofu_closure_b;
#[path = "impl_tofu_content.rs"]
mod impl_tofu_content;
#[path = "impl_tofu_detect.rs"]
mod impl_tofu_detect;
#[path = "impl_tofu_diagnostics.rs"]
mod impl_tofu_diagnostics;
#[path = "impl_tofu_effective.rs"]
mod impl_tofu_effective;
#[path = "impl_tofu_evidence.rs"]
mod impl_tofu_evidence;
#[path = "impl_tofu_family.rs"]
mod impl_tofu_family;
#[path = "impl_tofu_file_cache.rs"]
mod impl_tofu_file_cache;
#[path = "impl_tofu_fmt.rs"]
mod impl_tofu_fmt;
#[path = "impl_tofu_identity.rs"]
mod impl_tofu_identity;
#[path = "impl_tofu_infer.rs"]
mod impl_tofu_infer;
#[path = "impl_tofu_isolation.rs"]
mod impl_tofu_isolation;
#[path = "impl_tofu_lockfile.rs"]
mod impl_tofu_lockfile;
#[path = "impl_tofu_modules.rs"]
mod impl_tofu_modules;
#[path = "impl_tofu_parser.rs"]
mod impl_tofu_parser;
#[path = "impl_tofu_propose.rs"]
mod impl_tofu_propose;
#[path = "impl_tofu_provider_inputs.rs"]
mod impl_tofu_provider_inputs;
#[path = "impl_tofu_qualify.rs"]
mod impl_tofu_qualify;
#[path = "impl_tofu_roots.rs"]
mod impl_tofu_roots;
#[path = "impl_tofu_select.rs"]
mod impl_tofu_select;
#[path = "impl_tofu_t27_select.rs"]
mod impl_tofu_t27_select;
#[path = "impl_tofu_units.rs"]
mod impl_tofu_units;
#[path = "impl_tofu_version.rs"]
mod impl_tofu_version;

/// Shared inline-fixture support (no fixture files outside `tests/`).
mod support {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Test outcome boxing every error type.
    pub(crate) type Outcome = Result<(), Box<dyn std::error::Error>>;

    /// Unique temporary directory removed on drop.
    pub(crate) struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        /// Create a fresh directory under the system temp area.
        pub(crate) fn create(label: &str) -> std::io::Result<Self> {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos());
            let path = std::env::temp_dir().join(format!("velnor-{label}-{nanos}-{id}"));
            std::fs::create_dir_all(&path)?;
            Ok(Self { path })
        }

        /// Borrow the directory path.
        pub(crate) fn path(&self) -> &Path {
            &self.path
        }

        /// Write `content` to `relative`, creating parents.
        pub(crate) fn write(&self, relative: &str, content: &str) -> std::io::Result<PathBuf> {
            let target = self.path.join(relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, content)?;
            Ok(target)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.path));
        }
    }

    /// Absolute path of the repo-root shared fixture `name`.
    pub(crate) fn fixture_dir(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
    }

    /// `(repo-relative path, text)` pairs for every file under `dir`.
    pub(crate) fn read_pairs(
        dir: &Path,
        prefix: &str,
    ) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
        let mut pairs = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(current) = stack.pop() {
            for entry in std::fs::read_dir(&current)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let relative = path
                    .strip_prefix(dir)?
                    .to_str()
                    .ok_or("non-utf8 fixture name")?
                    .replace('\\', "/");
                let text = std::fs::read_to_string(&path)?;
                let key = if prefix.is_empty() {
                    relative
                } else {
                    format!("{prefix}/{relative}")
                };
                pairs.push((key, text));
            }
        }
        pairs.sort();
        Ok(pairs)
    }
}
