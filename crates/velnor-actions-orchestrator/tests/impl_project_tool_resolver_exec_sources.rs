//! Resolver execution matrix, part two: source agreement, malformed files,
//! toolchain spellings, and which tools are read.

use super::impl_project_tool_resolver_exec::{RUST, fails, passes, run};
use crate::impl_common::TestResult;

#[test]
fn disagreeing_sources_fail_wherever_they_live() -> TestResult {
    let rust = |version: &str| format!("[tools]\nrust = \"{version}\"\n");
    for (other, text) in [
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"1.98.0\"\n".to_owned(),
        ),
        ("rust-toolchain", "1.98.0\n".to_owned()),
        (
            "rust-toolchain",
            "[toolchain]\nchannel = \"1.98.0\"\n".to_owned(),
        ),
        (".config/mise.toml", rust("1.98.0")),
        (".config/mise/conf.d/z.toml", rust("1.98.0")),
        ("mise.local.toml", rust("1.98.0")),
        (".tool-versions", "rust 1.98.0\n".to_owned()),
    ] {
        fails(
            &[("mise.toml", &rust("1.99.1")), (other, &text)],
            &[RUST],
            "declares",
        )?;
    }
    fails(
        &[(
            "mise.toml",
            "[tools]\nrust = \"1.99.1\"\n\"core:rust\" = \"1.98.0\"\n",
        )],
        &[RUST],
        "declares",
    )?;
    passes(
        &[
            ("mise.toml", &rust("1.99.1")),
            (
                "rust-toolchain.toml",
                "[toolchain]\nchannel = \"1.99.1\"\ncomponents = [\"rustfmt\"]\n",
            ),
            (".tool-versions", "rust 1.99.1\n"),
            (".config/mise/conf.d/a.toml", &rust("1.99.1")),
        ],
        &[RUST],
        "VELNOR_TOOL_RUST=1.99.1\n",
    )
}

#[test]
fn a_malformed_source_fails_instead_of_falling_back() -> TestResult {
    for broken in [
        "mise.toml",
        ".mise.toml",
        ".config/mise.toml",
        ".config/mise/conf.d/a.toml",
        "mise.local.toml",
    ] {
        fails(
            &[
                (broken, "[tools\nrust = \"1.99.1\"\n"),
                ("rust-toolchain.toml", "[toolchain]\nchannel = \"1.99.1\"\n"),
            ],
            &[RUST],
            "is not valid TOML",
        )?;
    }
    fails(
        &[("rust-toolchain.toml", "[toolchain\nchannel = \"1.99.1\"\n")],
        &[RUST],
        "is not valid TOML",
    )
}

#[test]
fn toolchain_files_in_every_spelling() -> TestResult {
    for (file, text) in [
        ("rust-toolchain.toml", "[toolchain]\nchannel = \"1.99.1\"\n"),
        ("rust-toolchain", "1.99.1\n"),
        ("rust-toolchain", "  1.99.1  \r\n"),
        ("rust-toolchain", "[toolchain]\nchannel = \"1.99.1\"\n"),
        (
            "rust-toolchain",
            "\n# pinned\n[toolchain]\nchannel = '1.99.1'\n",
        ),
    ] {
        passes(&[(file, text)], &[RUST], "VELNOR_TOOL_RUST=1.99.1\n")?;
    }
    for (file, text) in [
        ("rust-toolchain.toml", "[toolchain]\nchannel = \"stable\"\n"),
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"1.99.1-x86_64-unknown-linux-gnu\"\n",
        ),
        ("rust-toolchain", "stable\n"),
        ("rust-toolchain", "1.99.1\nVELNOR_TOOL_X=1\n"),
        ("rust-toolchain", "[toolchain]\nchannel = \"nightly\"\n"),
        ("rust-toolchain", ""),
    ] {
        fails(&[(file, text)], &[RUST], "exact X.Y.Z")?;
    }
    Ok(())
}

#[test]
fn an_undeclared_referenced_tool_fails_and_unreferenced_loose_tools_do_not() -> TestResult {
    fails(
        &[("mise.toml", "[tools]\njava = \"21\"\n")],
        &["VELNOR_TOOL_NEXTEST"],
        "no longer declares",
    )?;
    passes(
        &[(
            "mise.toml",
            "[tools]\njava = \"lts\"\nnode = \"20\"\nrust = \"1.99.1\"\n",
        )],
        &[RUST],
        "VELNOR_TOOL_RUST=1.99.1\n",
    )
}

#[test]
fn rustup_toolchain_is_exported_only_on_request() -> TestResult {
    let files = [("mise.toml", "[tools]\nrust = \"1.99.1\"\n")];
    let with = run(&files, &[], &[RUST], true)?;
    assert!(with.ok, "{}", with.stderr);
    assert_eq!(
        with.env,
        "VELNOR_TOOL_RUST=1.99.1\nRUSTUP_TOOLCHAIN=1.99.1\n"
    );
    Ok(())
}
