// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::ToolPin;
use velnor_rust::{DetectError, Reason, detect};

use crate::facts::{TestResult, lib, path, repo};

fn pin(requested: &[&str], locked: &[&str]) -> ToolPin {
    let owned = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect();
    ToolPin {
        requested: owned(requested),
        locked: owned(locked),
    }
}

#[test]
fn mise_files_become_the_requested_and_locked_versions_of_each_tool() -> TestResult {
    let config = "\
[tools]
rust = { version = \"1.99.1\", profile = \"minimal\" }
\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"
node = [\"20\", \"22\"]
local = { path = \"tools/local\" }
";
    let lock = "\
lockfile_version = 3

[[tools.rust]]
version = \"1.99.1\"
backend = \"core:rust\"

[tools.rust.\"platforms.linux-x64\"]
checksum = \"sha256:ab\"

[[tools.\"aqua:nextest-rs/nextest/cargo-nextest\"]]
version = \"0.9.151\"
";
    let view = repo(
        vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])],
        &[("mise.toml", config), ("mise.lock", lock)],
    )?;
    let tools = detect(&view)?.tools;
    assert_eq!(tools.configs, vec![path("mise.toml")?]);
    let found: Vec<(&str, &ToolPin)> = tools
        .tools
        .iter()
        .map(|(name, pin)| (name.as_str(), pin))
        .collect();
    assert_eq!(
        found,
        vec![
            (
                "aqua:nextest-rs/nextest/cargo-nextest",
                &pin(&["0.9.151"], &["0.9.151"])
            ),
            ("local", &pin(&[], &[])),
            ("node", &pin(&["20", "22"], &[])),
            ("rust", &pin(&["1.99.1"], &["1.99.1"])),
        ]
    );
    Ok(())
}

#[test]
fn mise_toml_and_the_hidden_mise_toml_are_merged_with_the_hidden_one_winning() -> TestResult {
    let both = repo(
        vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])],
        &[
            ("mise.toml", "[tools]\nbun = \"1.0.0\"\njq = \"1.7\"\n"),
            (".mise.toml", "[tools]\nbun = \"2.0.0\"\nyq = \"4\"\n"),
        ],
    )?;
    let tools = detect(&both)?.tools;
    assert_eq!(tools.configs, vec![path("mise.toml")?, path(".mise.toml")?]);
    let found: Vec<(&str, &ToolPin)> = tools
        .tools
        .iter()
        .map(|(name, pin)| (name.as_str(), pin))
        .collect();
    assert_eq!(
        found,
        vec![
            ("bun", &pin(&["2.0.0"], &[])),
            ("jq", &pin(&["1.7"], &[])),
            ("yq", &pin(&["4"], &[])),
        ]
    );
    let mut hidden = both;
    hidden.files.remove(&path("mise.toml")?);
    let tools = detect(&hidden)?.tools;
    assert_eq!(tools.configs, vec![path(".mise.toml")?]);
    assert_eq!(tools.tools.get("bun"), Some(&pin(&["2.0.0"], &[])));
    Ok(())
}

#[test]
fn a_malformed_mise_file_is_an_error_naming_that_file() -> TestResult {
    let value = |key, value: &str| Reason::InvalidValue {
        key,
        value: value.to_owned(),
    };
    let cases = [
        ("mise.toml", "tools = 3\n", value("tools", "3")),
        ("mise.toml", "[tools]\nbad = 1\n", value("tools", "bad")),
    ];
    for (file, text, reason) in cases {
        let view = repo(
            vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])],
            &[(file, text)],
        )?;
        assert_eq!(
            detect(&view),
            Err(DetectError::Invalid {
                path: path(file)?,
                reason
            })
        );
    }
    let view = repo(
        vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])],
        &[("mise.lock", "[tools\n")],
    )?;
    assert!(matches!(
        detect(&view),
        Err(DetectError::Invalid { path: found, reason: Reason::Syntax(_) })
            if found.as_str() == "mise.lock"
    ));
    Ok(())
}
