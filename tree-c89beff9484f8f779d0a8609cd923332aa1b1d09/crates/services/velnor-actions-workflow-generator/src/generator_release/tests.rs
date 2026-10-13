use super::manifest;
const REPOSITORY: &str = "tailrocks/velnor-new";
const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
mod pins;
use pins::{PINNED_MISE_ARGUMENTS, test_pins};

mod cli_tests;
mod fake_commands;
mod publisher_receipt_tests;
mod publisher_script;

use publisher_script::{
    Failure, Scratch, install_mock_gh, path_with_mock_gh, sha256, write_attestation_files,
};
