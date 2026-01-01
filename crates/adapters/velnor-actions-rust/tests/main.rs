//! Integration test entry point; cases live in the sibling files.
#[path = "impl_adapter_wire_rust.rs"]
mod impl_adapter_wire_rust;
#[path = "impl_rust.rs"]
mod impl_rust;
#[path = "impl_rust_argv.rs"]
mod impl_rust_argv;
#[path = "impl_rust_detect.rs"]
mod impl_rust_detect;
#[path = "impl_rust_f2a.rs"]
mod impl_rust_f2a;
#[path = "impl_rust_f2b.rs"]
mod impl_rust_f2b;
#[path = "impl_rust_gates.rs"]
mod impl_rust_gates;
#[path = "impl_rust_negative.rs"]
mod impl_rust_negative;
#[path = "impl_rust_nested.rs"]
mod impl_rust_nested;
#[path = "impl_rust_p03_identity.rs"]
mod impl_rust_p03_identity;
#[path = "impl_rust_profile_emit.rs"]
mod impl_rust_profile_emit;
#[path = "impl_rust_propose_parity.rs"]
mod impl_rust_propose_parity;
#[path = "impl_rust_tasks.rs"]
mod impl_rust_tasks;
#[path = "impl_rust_test_preparation.rs"]
mod impl_rust_test_preparation;
#[path = "impl_rust_test_targets.rs"]
mod impl_rust_test_targets;

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
}
