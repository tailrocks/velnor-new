// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::Toolchain;
use velnor_rust::{DetectError, Reason, detect};

use crate::facts::{TestResult, lib, repo};

fn toolchain(
    channel: &str,
    components: &[&str],
    targets: &[&str],
    profile: Option<&str>,
) -> Toolchain {
    let owned = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect();
    Toolchain {
        channel: Some(channel.to_owned()),
        path: None,
        components: owned(components),
        targets: owned(targets),
        profile: profile.map(str::to_owned),
    }
}

type ToolchainCase = (&'static str, &'static str, Result<Toolchain, Reason>);

fn toolchain_cases() -> Vec<ToolchainCase> {
    let both = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"];
    vec![
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"1.99.1\"\ntargets = [\n    \"aarch64-unknown-linux-gnu\",\n    \"x86_64-unknown-linux-gnu\",\n]\n",
            Ok(toolchain("1.99.1", &[], &both, None)),
        ),
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"minimal\"\ncomponents = [\"clippy\", \"rustfmt\"]\ntargets = [\"aarch64-unknown-linux-gnu\", \"x86_64-unknown-linux-gnu\"]\n",
            Ok(toolchain(
                "1.99.1",
                &["clippy", "rustfmt"],
                &both,
                Some("minimal"),
            )),
        ),
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"stable\"\ncomponents = [\"clippy\", \"rustfmt\",]\n",
            Ok(toolchain("stable", &["clippy", "rustfmt"], &[], None)),
        ),
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"stable\"\ncomponents = []\ntargets = []\n",
            Ok(toolchain("stable", &[], &[], None)),
        ),
        (
            "rust-toolchain.toml",
            "[toolchain]\n# the channel\nchannel = \"nightly-2026-11-01\"\ncomponents = [\n    # lint\n    \"clippy\",\n    \"rustfmt\", # format\n    # last\n]\n",
            Ok(toolchain(
                "nightly-2026-11-01",
                &["clippy", "rustfmt"],
                &[],
                None,
            )),
        ),
        (
            "rust-toolchain.toml",
            "components = [\"clippy\"]\n",
            Err(Reason::MissingTable("toolchain")),
        ),
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"stable\"\ntarget = [\"wasm32-unknown-unknown\"]\n",
            Err(Reason::UnknownKey("target".to_owned())),
        ),
        (
            "rust-toolchain.toml",
            "[toolchain]\ncomponents = [\"clippy\"]\n",
            Ok(Toolchain {
                channel: None,
                path: None,
                components: vec!["clippy".to_owned()],
                targets: Vec::new(),
                profile: None,
            }),
        ),
        (
            "rust-toolchain.toml",
            "[toolchain]\n",
            Err(Reason::MissingKey("toolchain.channel")),
        ),
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"stable\"\ntargets = \"wasm32-unknown-unknown\"\n",
            Err(Reason::InvalidValue {
                key: "toolchain.targets",
                value: "\"wasm32-unknown-unknown\"".to_owned(),
            }),
        ),
    ]
}

fn plain_toolchain_cases() -> Vec<ToolchainCase> {
    vec![
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"stable\"\npath = \"/opt/rust\"\n",
            Err(Reason::InvalidValue {
                key: "toolchain.path",
                value: "/opt/rust".to_owned(),
            }),
        ),
        (
            "rust-toolchain.toml",
            "[toolchain]\npath = \"/opt/rust\"\n",
            Ok(Toolchain {
                channel: None,
                path: Some("/opt/rust".to_owned()),
                components: Vec::new(),
                targets: Vec::new(),
                profile: None,
            }),
        ),
        (
            "rust-toolchain",
            "1.99.1\n",
            Ok(toolchain("1.99.1", &[], &[], None)),
        ),
        (
            "rust-toolchain",
            "  nightly  ",
            Ok(toolchain("nightly", &[], &[], None)),
        ),
        (
            "rust-toolchain",
            "[toolchain]\nchannel = \"1.99.1\"\n",
            Ok(toolchain("1.99.1", &[], &[], None)),
        ),
        (
            "rust-toolchain",
            "[toolchain]\n",
            Err(Reason::InvalidValue {
                key: "toolchain.channel",
                value: "[toolchain]".to_owned(),
            }),
        ),
        (
            "rust-toolchain",
            "",
            Err(Reason::MissingKey("toolchain.channel")),
        ),
        (
            "rust-toolchain",
            "1.99 stable\n",
            Err(Reason::InvalidValue {
                key: "toolchain.channel",
                value: "1.99 stable".to_owned(),
            }),
        ),
    ]
}

#[test]
fn toolchain_files_become_the_toolchain_fact() -> TestResult {
    for (file, text, expected) in toolchain_cases().into_iter().chain(plain_toolchain_cases()) {
        let facts = detect(&repo(
            vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])],
            &[(file, text)],
        )?);
        let found = match facts {
            Ok(facts) => Ok(facts.toolchain),
            Err(DetectError::Invalid {
                path: found,
                reason,
            }) if found.as_str() == file => Err(reason),
            Err(other) => return Err(format!("{file}: {other:?}").into()),
        };
        assert_eq!(found, expected.map(Some), "{file}: {text:?}");
    }
    Ok(())
}

#[test]
fn a_plain_toolchain_file_wins_over_the_toml_file_as_in_rustup() -> TestResult {
    let facts = detect(&repo(
        vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])],
        &[
            ("rust-toolchain", "1.98.0\n"),
            ("rust-toolchain.toml", "[toolchain]\nchannel = \"1.99.1\"\n"),
        ],
    )?)?;
    assert_eq!(facts.toolchain, Some(toolchain("1.98.0", &[], &[], None)));
    assert_eq!(
        detect(&repo(
            vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])],
            &[]
        )?)?
        .toolchain,
        None
    );
    Ok(())
}
