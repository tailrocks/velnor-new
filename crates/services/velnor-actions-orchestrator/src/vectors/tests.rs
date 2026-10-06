use super::*;
use velnor_actions_rust::{CompileDriver, TestRunner};

/// Owned argv expectation from literals.
fn argv_of(parts: &[&str]) -> Vec<String> {
    parts.iter().map(ToString::to_string).collect()
}

mod vectors_tests;
