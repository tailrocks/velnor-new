//! Bounded, run-bound admission of hosted qualification predecessor receipts.

mod lineage;
mod request;
mod staged;

pub use request::resolve_qualification_admission;
pub use staged::load_qualification_admission;

/// Fixed staged-admission filename adjacent to the private plan request.
pub const QUALIFICATION_ADMISSION_FILENAME: &str = "qualification-admission.json";
