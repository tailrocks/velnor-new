// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::weaken_scope::{facts, introduced_files, named};

const MISE: &str =
    "[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\nmr-boxington = \"v1.30.0\"\n";

fn wrapper(body: &str) -> String {
    format!("{MISE}\n[wrappers.cargo]\n{body}")
}

#[test]
fn the_standard_mbx_cargo_wrapper_is_accepted_and_names_nothing() -> TestResult {
    let standard = wrapper("command = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n");
    assert_eq!(
        named(&[("mise.toml", &standard)], false)?,
        Vec::<String>::new()
    );
    Ok(())
}

#[test]
fn any_other_cargo_wrapper_is_still_named() -> TestResult {
    for body in [
        // A lookalike that only names mbx, or runs something else.
        "command = \"mbx\"\n",
        "command = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"0\" }\n",
        "command = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\", RUSTDOC = \"true\" }\n",
        "command = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\nargs = [\"--no-cache\"]\n",
        "command = \"sccache\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n",
        "command = \"/tmp/mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n",
        "command = \"true\"\n",
    ] {
        let named = named(&[("mise.toml", &wrapper(body))], false)?;
        assert_eq!(named.len(), 1, "{body}: {named:?}");
        assert!(named[0].contains("runs cargo through"), "{body}: {named:?}");
    }
    Ok(())
}

#[test]
fn the_standard_name_is_only_exempt_for_cargo_itself() -> TestResult {
    let text = format!(
        "{MISE}\n[wrappers.cargo-nextest]\ncommand = \"mbx\"\nenv = {{ MBX_CARGO_SHIM_MODE = \"1\" }}\n"
    );
    let named = named(&[("mise.toml", &text)], false)?;
    assert_eq!(named.len(), 1, "{named:?}");
    assert!(
        named[0].contains("runs cargo-nextest through \"mbx\""),
        "{named:?}"
    );
    Ok(())
}

#[test]
fn changing_the_standard_wrapper_into_a_different_one_is_new() -> TestResult {
    let after = facts(&[(
        "mise.toml",
        &wrapper("command = \"sccache\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n"),
    )])?;
    assert_eq!(
        introduced_files(
            &after,
            &["mise.toml"],
            &[(
                "mise.toml",
                &wrapper("command = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n"),
            )],
        )?,
        ["mise.toml"]
    );
    Ok(())
}
