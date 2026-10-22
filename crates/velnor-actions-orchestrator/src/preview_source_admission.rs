//! Two-phase, source-bound preview admission modules.

#[path = "preview_source_admission_finalization.rs"]
mod finalization;
#[path = "preview_source_admission_policy.rs"]
mod policy;
#[path = "preview_source_admission_selection.rs"]
mod selection;
#[path = "preview_source_admission_support.rs"]
mod support;

const MAX_MANIFEST_CLOSURE_BYTES: usize = 8 * 1024 * 1024;

pub use finalization::{FINALIZE_PREVIEW_SOURCE_OP, finalize_preview_source_from_env};
pub use selection::select_preview_source_from_env;
