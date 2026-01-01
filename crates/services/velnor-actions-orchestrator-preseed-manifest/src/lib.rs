//! Fresh-helper pre-seed manifest writer, extracted from the orchestrator.
//!
//! [`write_preseed_manifest`] resolves its six inputs from the process
//! environment and delegates to [`write_preseed_manifest_to`], the
//! explicit-input writer that hashes the fresh helper binary, stages a
//! copy beside the manifest, and emits the exact §4.4 JSON.

mod preseed_manifest;

pub use preseed_manifest::MANIFEST_FILE;
pub use preseed_manifest::PRESEED_BINARY_ENV;
pub use preseed_manifest::PRESEED_MANIFEST_OP;
pub use preseed_manifest::PRESEED_OUT_ENV;
pub use preseed_manifest::PRESEED_TARGET_ENV;
pub use preseed_manifest::PRESEED_TOOLCHAIN_ENV;
pub use preseed_manifest::write_preseed_manifest;
pub use preseed_manifest::write_preseed_manifest_to;
