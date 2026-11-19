// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_gen::{GenerateError, expected_files};
use velnor_model::{ChangeSet, RepoFacts};
use velnor_rust::{DetectError, detect, introduced};

use crate::facts::{TestResult, lib, path, repo};
use crate::pipeline::HELPER_LOCK;

const MISE: &str =
    "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n";

fn facts_with(files: &[(&str, &str)]) -> Result<RepoFacts, Box<dyn std::error::Error>> {
    let view = repo(
        vec![("Cargo.toml", vec![lib("foo", "crates/foo/Cargo.toml")?])],
        files,
    )?;
    Ok(detect(&view)?)
}

fn named(files: &[(&str, &str)]) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    Ok(facts_with(files)?
        .weakenings
        .iter()
        .map(ToString::to_string)
        .collect())
}

#[test]
fn a_setting_that_makes_a_check_do_less_is_accepted_and_named() -> TestResult {
    let cases: [(&str, &str, &str, &str); 19] = [
        (
            ".velnor/rust-ci.toml",
            "exclude = [\"tools/Cargo.toml\"]\n",
            "exclude = \"tools/Cargo.toml\"",
            "leaves tools/Cargo.toml out of the checks",
        ),
        (
            ".velnor/rust-ci.toml",
            "non_inputs = [\"data/**\"]\n",
            "non_inputs = \"data/**\"",
            "stops a change to data/** from selecting any package",
        ),
        (
            "crates/foo/Cargo.toml",
            "[package]\nname = \"foo\"\n\n[lib]\ndoctest = false\n",
            "[lib] doctest = false",
            "turns off doctests for foo",
        ),
        (
            ".config/nextest.toml",
            "[scripts.wrapper.skip]\ncommand = \"true\"\n\n[[profile.default.scripts]]\nfilter = \"all()\"\nrun-wrapper = \"skip\"\n",
            "profile.default.scripts run-wrapper = \"skip\" command = \"true\" filter = \"all()\"",
            "runs test binaries through the wrapper skip, command \"true\"",
        ),
        (
            ".cargo/config.toml",
            "[target.x86_64-unknown-linux-gnu]\nrunner = \"true\"\n",
            "target.x86_64-unknown-linux-gnu.runner = \"true\"",
            "runs test binaries through \"true\"",
        ),
        (
            ".cargo/config.toml",
            "[build]\nrustdocflags = [\"--test-args\", \"zzz\"]\n",
            "rustdocflags [\"--test-args\", \"zzz\"] with --test-args",
            "passes --test-args to rustdoc, which can leave out every doctest",
        ),
        (
            ".velnor/rust-ci.toml",
            "[[inputs]]\nglob = \"data/**\"\npackages = [\"foo\"]\n",
            "inputs = \"data/**\" packages = [\"foo\"]",
            "limits a change to data/** to selecting only foo",
        ),
        (
            ".cargo/config.toml",
            "[build]\nrustdocflags = [\"--no-run\"]\n",
            "rustdocflags [\"--no-run\"] with --no-run",
            "passes --no-run to rustdoc, which compiles every doctest without running it",
        ),
        (
            "mise.toml",
            "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\n[wrappers.cargo]\ncommand = \"mbx\"\n",
            "[wrappers.cargo] command = \"mbx\"",
            "runs cargo through \"mbx\"",
        ),
        (
            ".cargo/config.toml",
            "[build]\nrustc-wrapper = \"sccache\"\n",
            "build.rustc-wrapper = \"sccache\"",
            "runs the compiler through \"sccache\"",
        ),
        (
            ".cargo/config.toml",
            "[build]\nrustc-workspace-wrapper = \"sccache\"\n",
            "build.rustc-workspace-wrapper = \"sccache\"",
            "runs the compiler through \"sccache\"",
        ),
        (
            "mise.toml",
            "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\n[wrappers.cargo-nextest]\ncommand = \"true\"\n",
            "[wrappers.cargo-nextest] command = \"true\"",
            "runs cargo-nextest through \"true\"",
        ),
        (
            ".cargo/config.toml",
            "[build]\nrustdoc = \"/bin/true\"\n",
            "build.rustdoc = \"/bin/true\"",
            "runs \"/bin/true\" instead of rustdoc",
        ),
        (
            "crates/foo/Cargo.toml",
            "[package]\nname = \"foo\"\n\n[lib]\ntest = false\n",
            "[lib] test = false",
            "leaves the library unit tests of foo out of the test run",
        ),
        (
            "crates/foo/Cargo.toml",
            "[package]\nname = \"foo\"\nautotests = false\n",
            "autotests = false",
            "stops foo from finding the files in tests/ as tests",
        ),
        (
            "crates/foo/Cargo.toml",
            "[package]\nname = \"foo\"\n\n[[test]]\nname = \"slow\"\ntest = false\n",
            "[[test]] name = \"slow\" test = false",
            "leaves the test target slow of foo out of the test run",
        ),
        (
            "crates/foo/Cargo.toml",
            "[package]\nname = \"foo\"\n\n[[bin]]\nname = \"tool\"\npath = \"src/tool.rs\"\ntest = false\n",
            "[[bin]] name = \"tool\" test = false",
            "leaves the bin target tool of foo out of the test run",
        ),
        (
            ".config/nextest.toml",
            "[profile.default]\ndefault-filter = \"not all()\"\n",
            "profile.default.default-filter = \"not all()\"",
            "limits a plain nextest run to the tests matching \"not all()\"; the nextest step Velnor generates ignores it",
        ),
        (
            ".config/nextest.toml",
            "[profile.ci]\ndefault-filter = \"none()\"\n",
            "profile.ci.default-filter = \"none()\"",
            "limits a plain nextest run to the tests matching \"none()\"; the nextest step Velnor generates ignores it",
        ),
    ];
    for (file, text, setting, effect) in cases {
        let files = vec![("mise.toml", MISE), (file, text)];
        let mut view = repo(
            vec![
                ("Cargo.toml", vec![lib("foo", "crates/foo/Cargo.toml")?]),
                ("tools/Cargo.toml", vec![lib("t", "tools/t/Cargo.toml")?]),
            ],
            &files,
        )?;
        view.tracked_paths.insert(path(file)?);
        let facts = detect(&view)?;
        let want = format!("{file}: {setting}: {effect}");
        assert_eq!(
            facts
                .weakenings
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            std::slice::from_ref(&want),
            "{file}: {text}"
        );
        let lock = path(".velnor/rust-ci.lock")?;
        view.tracked_paths.insert(lock.clone());
        view.files.insert(
            lock,
            String::from_utf8(HELPER_LOCK.to_vec())?
                .replace("0.1.79", "0.1.81")
                .into_bytes(),
        );
        assert!(expected_files(&view).is_ok(), "{want}");
    }
    Ok(())
}

#[test]
fn ordinary_settings_name_nothing() -> TestResult {
    let none = named(&[
        (
            "mise.toml",
            &format!("{MISE}\n[wrappers.node]\ncommand = \"x\"\n"),
        ),
        (
            ".cargo/config.toml",
            "[build]\nrustflags = [\"-C\", \"debuginfo=0\"]\nrustdocflags = [\"--cfg\", \"docsrs\"]\n",
        ),
        (
            ".config/nextest.toml",
            "[profile.default]\nretries = 0\n\n[profile.ci]\nfail-fast = false\n\n[scripts.wrapper.unused]\ncommand = \"true\"\n",
        ),
        (
            "crates/foo/Cargo.toml",
            "[package]\nname = \"foo\"\nautotests = true\n\n[lib]\ndoctest = true\ntest = true\n\n[[test]]\nname = \"t\"\n",
        ),
        (".velnor/rust-ci.toml", "timeout_minutes = 20\n"),
    ])?;
    assert!(none.is_empty(), "{none:?}");
    Ok(())
}

#[test]
fn a_setting_velnor_cannot_see_or_that_cancels_its_flags_is_still_rejected() -> TestResult {
    for (file, text) in [
        (".cargo/config.toml", "include = [\"more.toml\"]\n"),
        (
            ".cargo/config.toml",
            "[build]\nrustflags = [\"--cap-lints\", \"allow\"]\n",
        ),
        (
            ".cargo/config.toml",
            "[env]\nRUSTC_WRAPPER = { value = \"true\", force = true }\n",
        ),
        (
            "mise.toml",
            "[hooks]\npostinstall = \"echo\"\n\n[tools]\nrust = \"1.99.1\"\n",
        ),
        (
            "mise.toml",
            "[env]\nANY = \"x\"\n\n[tools]\nrust = \"1.99.1\"\n",
        ),
    ] {
        let view = repo(
            vec![("Cargo.toml", vec![lib("foo", "crates/foo/Cargo.toml")?])],
            &[(file, text)],
        )?;
        let mut view = view;
        view.tracked_paths.insert(path(file)?);
        assert!(
            matches!(
                expected_files(&view),
                Err(GenerateError::Detect(DetectError::Invalid { .. }))
            ),
            "{file}: {text}"
        );
    }
    Ok(())
}

#[test]
fn only_a_weakening_a_change_adds_is_new() -> TestResult {
    let doctests_off = "[package]\nname = \"foo\"\n\n[lib]\ndoctest = false\n";
    let runner = "[target.x86_64-unknown-linux-gnu]\nrunner = \"true\"\n";
    let facts = facts_with(&[
        ("crates/foo/Cargo.toml", doctests_off),
        (".cargo/config.toml", runner),
    ])?;
    let run = |paths: &[&str],
               base: &[(&str, &str)]|
     -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let changes = ChangeSet {
            paths: paths
                .iter()
                .map(|p| path(p))
                .collect::<Result<BTreeSet<_>, _>>()?,
            base_files: base
                .iter()
                .map(|(p, t)| Ok((path(p)?, (*t).to_owned())))
                .collect::<Result<BTreeMap<_, _>, Box<dyn std::error::Error>>>()?,
            head_files: BTreeMap::new(),
        };
        Ok(introduced(&facts, &changes)
            .iter()
            .map(|w| w.file.to_string())
            .collect())
    };
    assert_eq!(run(&[], &[])?, Vec::<String>::new());
    assert_eq!(run(&["README.md"], &[])?, Vec::<String>::new());
    assert_eq!(
        run(
            &["crates/foo/Cargo.toml", ".cargo/config.toml"],
            &[("crates/foo/Cargo.toml", "[package]\nname = \"foo\"\n")]
        )?,
        [".cargo/config.toml", "crates/foo/Cargo.toml"]
    );
    assert_eq!(
        run(
            &["crates/foo/Cargo.toml", ".cargo/config.toml"],
            &[
                ("crates/foo/Cargo.toml", doctests_off),
                (".cargo/config.toml", runner)
            ]
        )?,
        Vec::<String>::new()
    );
    assert_eq!(
        run(
            &["crates/foo/Cargo.toml"],
            &[(
                "crates/foo/Cargo.toml",
                "[package]\nname = \"foo\"\n\n[lib]\ndoctest = true\n"
            )]
        )?,
        ["crates/foo/Cargo.toml"]
    );
    Ok(())
}

#[test]
fn a_rustdoc_flag_value_that_contains_a_bypass_flag_is_not_a_bypass() -> TestResult {
    let names = named(&[(
        ".cargo/config.toml",
        "[build]\nrustdocflags = [\"--cfg\", \"mode=\\\"--no-run\\\"\", \"--test-args-note\"]\n",
    )])?;
    assert!(
        names.iter().all(|name| !name.contains("rustdocflags")),
        "{names:?}"
    );
    Ok(())
}

#[test]
fn an_empty_compiler_wrapper_resets_cargo_and_is_not_named() -> TestResult {
    let files = [(
        ".cargo/config.toml",
        "[build]\nrustc-wrapper = \"\"\nrustc-workspace-wrapper = \"\"\n",
    )];
    assert_eq!(named(&files)?, Vec::<String>::new());
    Ok(())
}
