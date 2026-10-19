//! Build job facts from one exact, bound reservation.

mod model;
mod reader;
#[cfg(test)]
mod reader_tests;

pub(crate) use model::BoundJobFactsSnapshot;
pub(crate) use reader::{JobFactsReadContext, read_job_facts_snapshot};
