// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod compat;
mod tracking;

pub(super) use tracking::git_add;

use super::prepare::{MISE, Setup, prepare, reported};

#[test]
fn prepare_requires_git_to_verify_target_ownership() {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.repo_cargo_config = Some("[profile.dev.package.\"*\"]\ndebug = true\n");
    setup.plain = true;
    let run = prepare("compat-plain", &setup);
    assert!(!run.output.status.success(), "{:?}", run.output);
    assert!(String::from_utf8_lossy(&run.output.stderr).contains("git"));
    assert_eq!(
        (run.env_file, run.outputs, run.velnor_config),
        (String::new(), String::new(), String::new())
    );
}

#[test]
fn rust_before_1_71_gets_numeric_debug_values() {
    let mut setup = Setup::new(reported("1.70.0", Some("0.9.151")));
    setup.release = "1.70.0";
    setup.mise_toml = MISE.replace("1.99.1", "1.70.0");
    let run = prepare("old-rust", &setup);
    assert!(run.output.status.success(), "{:?}", run.output);
    assert!(
        run.env_file.contains("CARGO_PROFILE_DEV_DEBUG=1\n"),
        "{}",
        run.env_file
    );
    assert!(run.env_file.contains("CARGO_PROFILE_TEST_DEBUG=1\n"));
    assert!(
        run.velnor_config.contains("debug = 0\n"),
        "{}",
        run.velnor_config
    );
    assert!(!run.velnor_config.contains("\"none\""));
}
