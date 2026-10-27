// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::{PRIVATE_FETCH_TOKEN_ENV, PRIVATE_TOKEN_OUTPUT_EXPR, StepKind};
use velnor_actions_mise::{PinnedTool, ToolHomes, project_tool_expression, project_tool_shell_ref};

use super::{fail, reject_parts, render, valid_parts};

pub(super) fn check_generated_toolchain_expressions() -> Result<(), Box<dyn Error>> {
    let homes = ToolHomes::runner_temp();
    for toolchain in [
        project_tool_shell_ref(PinnedTool::Rust),
        project_tool_expression(PinnedTool::Rust),
    ] {
        let mut parts = valid_parts();
        parts.toolchain.extend([
            (
                "MISE_RUSTUP_HOME".to_owned(),
                homes.rustup_home().to_owned(),
            ),
            ("MISE_CARGO_HOME".to_owned(), homes.cargo_home().to_owned()),
            ("RUSTUP_TOOLCHAIN".to_owned(), toolchain),
        ]);
        let steps = render(&parts, &["cargo", "release-assets"])?;
        let StepKind::Internal { env, .. } = &steps[1].kind else {
            return Err(fail("generated private fetch step kind drifted"));
        };
        for (key, value) in &parts.toolchain {
            if env.get(key) != Some(value) {
                return Err(fail("generated private fetch toolchain changed"));
            }
        }
        if env.get(PRIVATE_FETCH_TOKEN_ENV).map(String::as_str) != Some(PRIVATE_TOKEN_OUTPUT_EXPR) {
            return Err(fail("generated private fetch token binding changed"));
        }
    }
    Ok(())
}

pub(super) fn check_rejects_unowned_toolchain_expressions() -> Result<(), Box<dyn Error>> {
    for (key, value) in [
        ("MISE_CARGO_HOME", "${{ runner.temp }}/velnor/rustup"),
        ("MISE_RUSTUP_HOME", "${{ runner.temp }}/velnor/cargo"),
        ("RUSTUP_HOME", "${{ runner.temp }}/velnor/rustup"),
        ("CARGO_HOME", "${{ runner.temp }}/velnor/cargo"),
        ("MISE_NO_CONFIG", "${{ env.VELNOR_TOOL_RUST }}"),
        ("MISE_RUSTUP_HOME", "${{ runner.temp }}/velnor/rustup/extra"),
        ("RUSTUP_TOOLCHAIN", "${{ env.VELNOR_TOOL_RUST }}-nightly"),
        ("MISE_RUSTUP_HOME", "${{ secrets.FIXTURE_APP_PRIVATE_KEY }}"),
        (
            "MISE_RUSTUP_HOME",
            "${{ secrets['FIXTURE_APP_PRIVATE_KEY'] }}",
        ),
        ("RUSTUP_TOOLCHAIN", "${{ github.token }}"),
        ("RUSTUP_TOOLCHAIN", "${{ github['token'] }}"),
        ("MISE_CARGO_HOME", "${{ steps.deps-token.outputs.token }}"),
        (
            "MISE_CARGO_HOME",
            "${{ steps['deps-token']['outputs']['token'] }}",
        ),
        (
            "MISE_CARGO_HOME",
            "${{ runner.temp }}/velnor/cargo${{ secrets.KEY }}",
        ),
    ] {
        let mut parts = valid_parts();
        parts.toolchain.insert(key.to_owned(), value.to_owned());
        reject_parts("unowned toolchain expression", &parts, &["cargo"])?;
    }
    Ok(())
}
