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
        ..ToolPin::default()
    }
}

#[test]
fn mise_files_become_the_requested_and_locked_versions_of_each_tool() -> TestResult {
    let config = "\
[tools]
rust = { version = \"1.99.1\", profile = \"minimal\" }
\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"
node = [\"20\", \"22\"]
local = { path = \"/opt/local\" }
edge = { ref = \"main\" }
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
            ("edge", &pin(&["ref:main"], &[])),
            ("local", &pin(&["path:/opt/local"], &[])),
            ("node", &pin(&["20", "22"], &[])),
            ("rust", &pin(&["1.99.1"], &["1.99.1"])),
        ]
    );
    Ok(())
}

#[test]
fn an_os_filtered_request_keeps_its_filter_and_an_unfiltered_duplicate_drops_it() -> TestResult {
    let config = "\
[tools]
go = [{ version = \"1.22\", os = [\"macos\"] }, { version = \"1.23\", os = \"linux\" }, \"1\"]
node = [{ version = \"20\", os = [\"linux\"] }, \"20\"]
";
    let view = repo(
        vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])],
        &[("mise.toml", config)],
    )?;
    let tools = detect(&view)?.tools;
    let go = tools.tools.get("go").ok_or("go is pinned")?;
    assert_eq!(go.requested, ["1.22", "1.23", "1"]);
    assert_eq!(go.os_filters.get("1.22"), Some(&vec!["macos".to_owned()]));
    assert_eq!(go.os_filters.get("1.23"), Some(&vec!["linux".to_owned()]));
    assert_eq!(go.os_filters.get("1"), None);
    let node = tools.tools.get("node").ok_or("node is pinned")?;
    assert!(node.os_filters.is_empty());
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
    let found: Vec<(&str, &[String])> = tools
        .tools
        .iter()
        .map(|(name, pin)| (name.as_str(), pin.requested.as_slice()))
        .collect();
    assert_eq!(
        found,
        vec![
            ("bun", ["2.0.0".to_owned()].as_slice()),
            ("jq", ["1.7".to_owned()].as_slice()),
            ("yq", ["4".to_owned()].as_slice()),
        ]
    );
    let mut hidden = both;
    hidden.files.remove(&path("mise.toml")?);
    let tools = detect(&hidden)?.tools;
    assert_eq!(tools.configs, vec![path(".mise.toml")?]);
    assert_eq!(
        tools.tools.get("bun").map(|pin| pin.requested.clone()),
        Some(vec!["2.0.0".to_owned()])
    );
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
        (
            "mise.toml",
            "[tools]\nlocal = { profile = \"x\" }\n",
            value("tools", "local"),
        ),
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
