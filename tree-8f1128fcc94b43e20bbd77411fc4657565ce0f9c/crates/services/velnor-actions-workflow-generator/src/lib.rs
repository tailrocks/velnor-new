//! Schema 2 generator release: Linux/macOS asset workflows and manifests.
//!
//! Pure and total: asset rendering contains no `std::fs`, `std::net`, or
//! process calls. Callers supply all discovered inputs.

#![forbid(unsafe_code)]

pub mod generator_release;
pub mod generator_release_pins;
pub mod request;

pub use generator_release_pins::GeneratorReleasePins;
pub use request::{MbxQualificationPins, Schema2WorkflowRequest};
