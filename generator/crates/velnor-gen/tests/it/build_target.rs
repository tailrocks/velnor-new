// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::RepoView;
use velnor_rust::{DetectError, Reason};

use crate::facts::{TestResult, path};
use crate::pipeline::view;

const SMALL: &str = "[build]\ntarget = \"thumbv6m-none-eabi\"\n";

fn with(roots: &[&str], configs: &[(&str, &str)]) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(roots, None)?;
    for (name, text) in configs {
        repo.tracked_paths.insert(path(name)?);
        repo.files.insert(path(name)?, text.as_bytes().to_vec());
    }
    if configs.iter().any(|(name, _)| *name == "mise.toml") {
        let lock = path("mise.lock")?;
        repo.tracked_paths.remove(&lock);
        repo.files.remove(&lock);
    }
    crate::facts::lock_pinned_tools(&mut repo)?;
    Ok(repo)
}

fn generate(repo: &RepoView) -> Result<(), GenerateError> {
    expected_with_notes(repo).map(|_| ())
}

fn refused(repo: &RepoView) -> Option<(String, Option<String>)> {
    match generate(repo) {
        Err(GenerateError::Detect(DetectError::Invalid { path, reason })) => Some((
            path.to_string(),
            match reason {
                Reason::UndeclaredTarget(name) => Some(name),
                Reason::InvalidValue { key, .. } => Some(key.to_owned()),
                Reason::HostTupleVersion(version) => Some(version),
                _ => None,
            },
        )),
        _ => None,
    }
}

#[test]
fn the_host_tuple_target_is_the_runner_host_and_needs_no_declaration() -> TestResult {
    for text in [
        "[build]\ntarget = \"host-tuple\"\n",
        "[build]\ntarget = [\"host-tuple\"]\n",
    ] {
        let repo = with(&["Cargo.toml"], &[(".cargo/config.toml", text)])?;
        assert!(generate(&repo).is_ok(), "{text}");
    }
    Ok(())
}

#[test]
fn the_host_tuple_target_requires_rust_191_or_newer() -> TestResult {
    for target in [
        "[build]\ntarget = \"host-tuple\"\n",
        "[build]\ntarget = [\"host-tuple\"]\n",
    ] {
        for (mise, toolchain) in [
            (
                "[tools]\nrust = \"1.90.0\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n",
                None,
            ),
            (
                "[tools]\nrust = \"1.90.0\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n",
                Some("1.90.0\n"),
            ),
            (
                "[tools]\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n",
                Some("1.90.0\n"),
            ),
        ] {
            let mut configs = vec![(".cargo/config.toml", target), ("mise.toml", mise)];
            if let Some(channel) = toolchain {
                configs.push(("rust-toolchain", channel));
            }
            let repo = with(&["Cargo.toml"], &configs)?;
            assert_eq!(
                refused(&repo),
                Some((".cargo/config.toml".to_owned(), Some("1.90.0".to_owned()))),
                "{target} {mise} {toolchain:?}"
            );
        }
        for (mise, toolchain) in [
            (
                "[tools]\nrust = \"1.91.0\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n",
                None,
            ),
            (
                "[tools]\nrust = \"1.91.0\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n",
                Some("1.91.0\n"),
            ),
            (
                "[tools]\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n",
                Some("1.91.0\n"),
            ),
        ] {
            let mut configs = vec![(".cargo/config.toml", target), ("mise.toml", mise)];
            if let Some(channel) = toolchain {
                configs.push(("rust-toolchain", channel));
            }
            let repo = with(&["Cargo.toml"], &configs)?;
            let result = generate(&repo);
            assert!(result.is_ok(), "{target} {mise} {toolchain:?}: {result:?}");
        }
    }
    Ok(())
}

#[test]
fn an_old_dated_nightly_cannot_use_the_host_tuple_target() -> TestResult {
    let repo = with(
        &["Cargo.toml"],
        &[
            (".cargo/config.toml", "[build]\ntarget = \"host-tuple\"\n"),
            (
                "mise.toml",
                "[tools]\nrust = \"nightly-2025-01-01\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n",
            ),
        ],
    )?;
    assert_eq!(
        refused(&repo),
        Some((
            ".cargo/config.toml".to_owned(),
            Some("nightly-2025-01-01".to_owned())
        ))
    );
    Ok(())
}

#[test]
fn opaque_fixed_rust_selectors_cannot_use_the_host_tuple_target() -> TestResult {
    for selector in ["custom-compiler-1", "nightly-2025-99-99"] {
        let mise = format!(
            "[tools]\nrust = \"{selector}\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n"
        );
        let repo = with(
            &["Cargo.toml"],
            &[
                (".cargo/config.toml", "[build]\ntarget = \"host-tuple\"\n"),
                ("mise.toml", &mise),
            ],
        )?;
        assert_eq!(
            refused(&repo),
            Some((".cargo/config.toml".to_owned(), Some(selector.to_owned()))),
            "{selector}"
        );
    }
    Ok(())
}

#[test]
fn a_supported_pre_release_nightly_can_use_the_host_tuple_target() -> TestResult {
    let repo = with(
        &["Cargo.toml"],
        &[
            (".cargo/config.toml", "[build]\ntarget = \"host-tuple\"\n"),
            (
                "mise.toml",
                "[tools]\nrust = \"nightly-2025-10-01\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n",
            ),
        ],
    )?;
    assert!(generate(&repo).is_ok());
    Ok(())
}

#[test]
fn floating_rust_channels_can_use_the_host_tuple_target() -> TestResult {
    for channel in ["stable", "beta", "nightly"] {
        let mise = format!(
            "[tools]\nrust = \"{channel}\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n"
        );
        let repo = with(
            &["Cargo.toml"],
            &[
                (".cargo/config.toml", "[build]\ntarget = \"host-tuple\"\n"),
                ("mise.toml", &mise),
            ],
        )?;
        assert!(generate(&repo).is_ok(), "{channel}");
    }
    Ok(())
}

#[test]
fn only_a_config_that_applies_to_a_root_names_its_targets() -> TestResult {
    for file in [
        "vendor/.cargo/config.toml",
        "lab/crates/foo/.cargo/config.toml",
    ] {
        let repo = with(&["lab/Cargo.toml"], &[(file, SMALL)])?;
        assert!(generate(&repo).is_ok(), "{file}");
    }
    let applied = with(&["lab/Cargo.toml"], &[("lab/.cargo/config.toml", SMALL)])?;
    assert_eq!(
        refused(&applied),
        Some((
            "lab/.cargo/config.toml".to_owned(),
            Some("thumbv6m-none-eabi".to_owned())
        ))
    );
    Ok(())
}

#[test]
fn a_nearer_string_replaces_an_ancestor_string_and_arrays_are_joined() -> TestResult {
    let host = "[build]\ntarget = \"host-tuple\"\n";
    let replaced = with(
        &["lab/Cargo.toml"],
        &[
            (".cargo/config.toml", SMALL),
            ("lab/.cargo/config.toml", host),
        ],
    )?;
    assert!(generate(&replaced).is_ok());
    let joined = with(
        &["lab/Cargo.toml"],
        &[
            (
                ".cargo/config.toml",
                "[build]\ntarget = [\"thumbv6m-none-eabi\"]\n",
            ),
            (
                "lab/.cargo/config.toml",
                "[build]\ntarget = [\"host-tuple\"]\n",
            ),
        ],
    )?;
    assert_eq!(
        refused(&joined),
        Some((
            ".cargo/config.toml".to_owned(),
            Some("thumbv6m-none-eabi".to_owned())
        ))
    );
    Ok(())
}

#[test]
fn cargo_target_config_types_must_match_across_applicable_configs() -> TestResult {
    for configs in [
        [
            (".cargo/config.toml", "[build]\ntarget = \"host-tuple\"\n"),
            (
                "lab/.cargo/config.toml",
                "[build]\ntarget = [\"host-tuple\"]\n",
            ),
        ],
        [
            (".cargo/config.toml", "[build]\ntarget = [\"host-tuple\"]\n"),
            (
                "lab/.cargo/config.toml",
                "[build]\ntarget = \"host-tuple\"\n",
            ),
        ],
    ] {
        let repo = with(&["lab/Cargo.toml"], &configs)?;
        assert_eq!(
            refused(&repo),
            Some((
                ".cargo/config.toml".to_owned(),
                Some("build.target".to_owned())
            )),
            "{configs:?}"
        );
    }
    for configs in [
        [
            (".cargo/config.toml", "[build]\ntarget = \"host-tuple\"\n"),
            (
                "lab/.cargo/config.toml",
                "[build]\ntarget = \"host-tuple\"\n",
            ),
        ],
        [
            (".cargo/config.toml", "[build]\ntarget = [\"host-tuple\"]\n"),
            (
                "lab/.cargo/config.toml",
                "[build]\ntarget = [\"host-tuple\"]\n",
            ),
        ],
    ] {
        assert!(
            generate(&with(&["lab/Cargo.toml"], &configs)?).is_ok(),
            "{configs:?}"
        );
    }
    Ok(())
}

#[test]
fn a_build_target_that_is_not_a_string_or_array_of_strings_is_refused() -> TestResult {
    for text in [
        "[build]\ntarget = 1\n",
        "[build]\ntarget = true\n",
        "[build.target]\nname = \"x\"\n",
        "[build]\ntarget = [1]\n",
    ] {
        let repo = with(&["Cargo.toml"], &[(".cargo/config.toml", text)])?;
        assert_eq!(
            refused(&repo),
            Some((
                ".cargo/config.toml".to_owned(),
                Some("build.target".to_owned())
            )),
            "{text}"
        );
    }
    Ok(())
}
