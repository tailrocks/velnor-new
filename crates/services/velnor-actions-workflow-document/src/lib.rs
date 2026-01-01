//! Workflow document assembly: lanes, matrix, and YAML emission.
//!
//! Pure and total: document rendering contains no `std::fs`, `std::net`,
//! or process calls. Callers supply all discovered inputs.

#![forbid(unsafe_code)]

pub mod artifact_matrix;
pub mod document;
pub mod document_lanes;
pub mod document_steps;
pub mod lane_share;
pub mod lane_share_sections;
pub mod lane_target;
pub mod matrix;
pub mod matrix_output_mode;
