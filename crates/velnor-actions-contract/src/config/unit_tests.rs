//! Unit-test modules for the config tables without a dedicated declaration in `mod.rs`.

use super::*;

#[path = "dockerfile_tests.rs"]
mod dockerfile_tests;
#[path = "images_tests.rs"]
mod images_tests;
#[path = "preview_policy_tests.rs"]
mod preview_policy_tests;
