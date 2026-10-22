use super::*;

#[path = "preview_production_test_support.rs"]
mod support;
use support::{assembly, policy, request};

#[path = "preview_production_channel_tests.rs"]
mod channel_tests;
#[path = "preview_production_identity_tests.rs"]
mod identity_tests;
#[path = "preview_production_parser_tests.rs"]
mod parser_tests;
