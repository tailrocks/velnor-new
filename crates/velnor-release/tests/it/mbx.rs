// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleasePlan;

use crate::fixture::{MISE_TOML, TestResult, files};

fn mbx_of(mise: &str, opt_out: bool) -> Result<bool, Box<dyn std::error::Error>> {
    let mut files = files();
    files.mise_tomls = vec![format!("{MISE_TOML}{mise}")];
    files.mbx_opt_out = opt_out;
    Ok(ReleasePlan::read(&files)?.mbx)
}

const OPTION: &str = "rust = { version = \"1.99.1\", mr_boxington = true }\n";

#[test]
fn only_the_exact_wrapper_and_the_rust_option_make_the_release_use_mbx() -> TestResult {
    for (name, text, expected) in [
        ("nothing", "", false),
        ("rust option", OPTION, true),
        (
            "core:rust option",
            "\"core:rust\" = { version = \"1.99.1\", mr_boxington = true }\n",
            true,
        ),
        (
            "exact wrapper",
            "\n[wrappers.cargo]\ncommand = \"mbx\"\n",
            true,
        ),
        (
            "wrapper by path",
            "\n[wrappers.cargo]\ncommand = \"./bin/mbx\"\n",
            false,
        ),
        (
            "wrapper in another directory",
            "\n[wrappers.cargo]\ncommand = \"/tmp/evil/mbx\"\n",
            false,
        ),
        (
            "wrapper with arguments",
            "\n[wrappers.cargo]\ncommand = \"mbx --x\"\n",
            false,
        ),
        (
            "option on another tool",
            "node = { version = \"24\", mr_boxington = true }\n",
            false,
        ),
        (
            "option in a list of requests",
            "node = [{ version = \"24\", mr_boxington = true }]\n",
            false,
        ),
    ] {
        assert_eq!(mbx_of(text, false)?, expected, "{name}");
    }
    Ok(())
}

#[test]
fn the_opt_out_turns_mbx_off_whatever_mise_declares() -> TestResult {
    for text in [
        OPTION,
        "\n[wrappers.cargo]\ncommand = \"mbx\"\n",
        "\n[wrappers.cargo]\ncommand = \"./bin/mbx\"\n",
    ] {
        assert!(!mbx_of(text, true)?, "{text}");
    }
    Ok(())
}
