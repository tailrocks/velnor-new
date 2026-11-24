// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::Invocation;

#[test]
fn a_command_local_target_dir_is_refused_for_every_subcommand_that_builds() {
    for subcommand in ["build", "run", "r", "bench", "rustc", "test", "check"] {
        let command: Vec<String> = [
            "cargo",
            subcommand,
            "--bin",
            "x",
            "--target-dir",
            "elsewhere",
        ]
        .map(str::to_owned)
        .to_vec();
        let refused = Invocation::new(&command)
            .validate_targets(Path::new("/repo"), &[], &[], &[], &[])
            .expect_err(subcommand);
        assert!(
            refused.to_string().contains("--target-dir"),
            "{subcommand}: {refused}"
        );
    }
}
