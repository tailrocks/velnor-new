// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_model::{
    DependencyKind, MemberDependency, PackageDependency, PackageName, PathGlob, RepoFacts,
    RepoPath, RepoView, RustMetadata, RustPackage, Target, TargetKind, TargetName, ToolPin,
    Toolchain,
};
use velnor_rust::{CONFIG_PATH, DetectError, Reason, detect};

use crate::golden::{load, samples};

type TestResult = Result<(), Box<dyn Error>>;

const MISE: &str = "[tools]\nrust = \"1.99.1\"\n";

fn path(text: &str) -> Result<RepoPath, Box<dyn Error>> {
    Ok(RepoPath::new(text)?)
}

fn package(
    name: &str,
    manifest: &str,
    dependencies: &[(&str, DependencyKind, &str)],
    targets: &[(&str, TargetKind)],
) -> Result<RustPackage, Box<dyn Error>> {
    Ok(RustPackage {
        name: PackageName::new(name)?,
        manifest: path(manifest)?,
        dependencies: dependencies
            .iter()
            .map(|(name, kind, manifest)| {
                Ok(PackageDependency {
                    name: PackageName::new(*name)?,
                    kind: *kind,
                    manifest: Some(path(manifest)?),
                })
            })
            .collect::<Result<_, Box<dyn Error>>>()?,
        targets: targets
            .iter()
            .map(|(name, kind)| {
                Ok(Target {
                    name: TargetName::new(*name)?,
                    kinds: vec![*kind],
                })
            })
            .collect::<Result<_, Box<dyn Error>>>()?,
    })
}

fn lib(name: &str, manifest: &str) -> Result<RustPackage, Box<dyn Error>> {
    package(name, manifest, &[], &[(name, TargetKind::Lib)])
}

fn repo(
    workspaces: Vec<(&str, Vec<RustPackage>)>,
    files: &[(&str, &str)],
) -> Result<RepoView, Box<dyn Error>> {
    let mut view = RepoView::default();
    for (manifest, members) in workspaces {
        let manifest = path(manifest)?;
        view.tracked_paths
            .insert(manifest.sibling(&path("Cargo.lock")?));
        view.tracked_paths.insert(manifest.clone());
        view.rust_metadata
            .insert(manifest, RustMetadata { members });
    }
    for (name, text) in [("mise.toml", MISE)].iter().chain(files) {
        view.tracked_paths.insert(path(name)?);
        view.files.insert(path(name)?, text.as_bytes().to_vec());
    }
    Ok(view)
}

type Shape = Vec<(String, Vec<(String, Option<String>)>)>;

fn shape(facts: &RepoFacts) -> Shape {
    facts
        .rust_roots
        .iter()
        .map(|root| {
            let members = root
                .members
                .iter()
                .map(|member| {
                    (
                        member.name.to_string(),
                        member.directory.as_ref().map(ToString::to_string),
                    )
                })
                .collect();
            (root.manifest.to_string(), members)
        })
        .collect()
}

fn root(
    manifest: &str,
    members: &[(&str, Option<&str>)],
) -> (String, Vec<(String, Option<String>)>) {
    (
        manifest.to_owned(),
        members
            .iter()
            .map(|(name, directory)| ((*name).to_owned(), directory.map(str::to_owned)))
            .collect(),
    )
}

#[test]
fn workspaces_and_standalone_packages_each_become_a_root_with_their_members() -> TestResult {
    let cases: Vec<(RepoView, Shape)> = vec![
        (
            repo(
                vec![(
                    "Cargo.toml",
                    vec![
                        lib("a", "crates/a/Cargo.toml")?,
                        lib("b", "crates/b/Cargo.toml")?,
                    ],
                )],
                &[],
            )?,
            vec![root(
                "Cargo.toml",
                &[("a", Some("crates/a")), ("b", Some("crates/b"))],
            )],
        ),
        (
            repo(
                vec![
                    ("Cargo.toml", vec![lib("a", "a/Cargo.toml")?]),
                    ("tools/Cargo.toml", vec![lib("t", "tools/t/Cargo.toml")?]),
                ],
                &[],
            )?,
            vec![
                root("Cargo.toml", &[("a", Some("a"))]),
                root("tools/Cargo.toml", &[("t", Some("tools/t"))]),
            ],
        ),
        (
            repo(
                vec![
                    ("Cargo.toml", vec![lib("outer", "Cargo.toml")?]),
                    (
                        "crates/inner/Cargo.toml",
                        vec![lib("inner", "crates/inner/x/Cargo.toml")?],
                    ),
                ],
                &[],
            )?,
            vec![
                root("Cargo.toml", &[("outer", None)]),
                root(
                    "crates/inner/Cargo.toml",
                    &[("inner", Some("crates/inner/x"))],
                ),
            ],
        ),
        (
            repo(
                vec![("solo/Cargo.toml", vec![lib("solo", "solo/Cargo.toml")?])],
                &[],
            )?,
            vec![root("solo/Cargo.toml", &[("solo", Some("solo"))])],
        ),
    ];
    for (view, expected) in cases {
        assert_eq!(shape(&detect(&view)?), expected);
    }
    Ok(())
}

#[test]
fn path_dependencies_are_edges_between_members_and_between_roots() -> TestResult {
    let helper = package(
        "velnor-ci",
        "ci-helper/crates/velnor-ci/Cargo.toml",
        &[
            (
                "velnor-model",
                DependencyKind::Normal,
                "generator/crates/velnor-model/Cargo.toml",
            ),
            (
                "support",
                DependencyKind::Dev,
                "ci-helper/crates/support/Cargo.toml",
            ),
            (
                "fixture",
                DependencyKind::Normal,
                "fixtures/fixture/Cargo.toml",
            ),
        ],
        &[],
    )?;
    let view = repo(
        vec![
            (
                "ci-helper/Cargo.toml",
                vec![
                    helper,
                    lib("support", "ci-helper/crates/support/Cargo.toml")?,
                ],
            ),
            (
                "generator/Cargo.toml",
                vec![lib(
                    "velnor-model",
                    "generator/crates/velnor-model/Cargo.toml",
                )?],
            ),
        ],
        &[],
    )?;
    let facts = detect(&view)?;
    let edges: Vec<(String, Vec<String>)> = facts
        .rust_roots
        .iter()
        .map(|root| {
            (
                root.manifest.to_string(),
                root.depends_on.iter().map(ToString::to_string).collect(),
            )
        })
        .collect();
    assert_eq!(
        edges,
        vec![
            (
                "ci-helper/Cargo.toml".to_owned(),
                vec!["generator/Cargo.toml".to_owned()]
            ),
            ("generator/Cargo.toml".to_owned(), vec![]),
        ]
    );
    let member = facts.rust_roots[0]
        .members
        .iter()
        .find(|member| member.name.as_str() == "velnor-ci")
        .ok_or("velnor-ci is not a member")?;
    assert_eq!(
        member.dependencies,
        vec![MemberDependency {
            name: PackageName::new("support")?,
            kind: DependencyKind::Dev,
        }]
    );
    Ok(())
}

#[test]
fn member_targets_are_kept_and_a_test_target_is_reported() -> TestResult {
    let tested = package(
        "tested",
        "tested/Cargo.toml",
        &[],
        &[
            ("tested", TargetKind::Lib),
            ("tool", TargetKind::Bin),
            ("derive", TargetKind::ProcMacro),
            ("build-script-build", TargetKind::BuildScript),
            ("smoke", TargetKind::Test),
        ],
    )?;
    let untested = package(
        "untested",
        "untested/Cargo.toml",
        &[],
        &[("untested", TargetKind::Lib), ("bench", TargetKind::Bench)],
    )?;
    let view = repo(vec![("Cargo.toml", vec![tested, untested])], &[])?;
    let facts = detect(&view)?;
    let members = &facts.rust_roots[0].members;
    let kinds: Vec<Vec<TargetKind>> = members[0]
        .targets
        .iter()
        .map(|target| target.kinds.clone())
        .collect();
    assert_eq!(
        kinds,
        vec![
            vec![TargetKind::Lib],
            vec![TargetKind::Bin],
            vec![TargetKind::ProcMacro],
            vec![TargetKind::BuildScript],
            vec![TargetKind::Test],
        ]
    );
    assert_eq!(
        members
            .iter()
            .map(velnor_model::RustMember::has_test_target)
            .collect::<Vec<_>>(),
        vec![true, false]
    );
    Ok(())
}

#[test]
fn a_root_without_a_committed_lockfile_is_an_error_unless_it_is_excluded() -> TestResult {
    let mut view = repo(
        vec![
            ("Cargo.toml", vec![lib("a", "a/Cargo.toml")?]),
            ("b/Cargo.toml", vec![lib("b", "b/b/Cargo.toml")?]),
        ],
        &[],
    )?;
    view.tracked_paths.remove(&path("b/Cargo.lock")?);
    assert_eq!(
        detect(&view),
        Err(DetectError::MissingLockfile {
            path: path("b/Cargo.lock")?,
            manifest: path("b/Cargo.toml")?,
        })
    );
    view.rust_metadata.remove(&path("b/Cargo.toml")?);
    view.tracked_paths.remove(&path("b/Cargo.lock")?);
    view.tracked_paths.insert(CONFIG_PATH);
    view.files
        .insert(CONFIG_PATH, b"exclude = [\"b\"]\n".to_vec());
    assert_eq!(
        shape(&detect(&view)?),
        vec![root("Cargo.toml", &[("a", Some("a"))])]
    );
    Ok(())
}

#[test]
fn an_exclude_glob_matches_a_path_or_anything_below_it() -> TestResult {
    let cases = [
        ("fixtures/**", "fixtures/a/Cargo.toml", true),
        ("fixtures/**", "fixtures/Cargo.toml", true),
        ("fixtures/**", "fixture/a/Cargo.toml", false),
        ("fixtures", "fixtures/a/b/Cargo.toml", true),
        ("fixtures", "fixtures2/Cargo.toml", false),
        (
            "crates/**/tests/fixtures/**",
            "crates/a/b/tests/fixtures/x/Cargo.toml",
            true,
        ),
        (
            "crates/**/tests/fixtures/**",
            "crates/a/tests/fixtures/Cargo.toml",
            true,
        ),
        (
            "crates/**/tests/fixtures/**",
            "crates/a/tests/other/Cargo.toml",
            false,
        ),
        ("crates/*/tests", "crates/a/tests/x/Cargo.toml", true),
        ("crates/*/tests", "crates/a/b/tests/Cargo.toml", false),
        ("crates/velnor-*", "crates/velnor-runner/Cargo.toml", true),
        ("crates/*-runner", "crates/velnor-core/Cargo.toml", false),
        ("Cargo.toml", "Cargo.toml", true),
        ("Cargo.toml", "a/Cargo.toml", false),
    ];
    for (glob, manifest, expected) in cases {
        assert_eq!(
            PathGlob::new(glob)?.matches(&path(manifest)?),
            expected,
            "{glob} against {manifest}"
        );
    }
    Ok(())
}

#[test]
fn an_exclude_entry_that_matches_no_manifest_is_an_error_naming_the_entry() -> TestResult {
    let mut view = repo(vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])], &[])?;
    view.tracked_paths.insert(CONFIG_PATH);
    view.files
        .insert(CONFIG_PATH, b"exclude = [\"fixtures/**\"]\n".to_vec());
    assert_eq!(
        detect(&view),
        Err(DetectError::Invalid {
            path: CONFIG_PATH,
            reason: Reason::UnmatchedExclude(PathGlob::new("fixtures/**")?),
        })
    );
    Ok(())
}

#[test]
fn an_unknown_config_key_is_an_error_naming_the_key() -> TestResult {
    let mut view = repo(vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])], &[])?;
    view.files.insert(
        CONFIG_PATH,
        b"timeout_minutes = 5\nexcludes = []\n".to_vec(),
    );
    assert_eq!(
        detect(&view),
        Err(DetectError::Invalid {
            path: CONFIG_PATH,
            reason: Reason::UnknownKey("excludes".to_owned()),
        })
    );
    Ok(())
}

#[test]
fn a_repository_without_a_mise_configuration_is_an_error() -> TestResult {
    let mut view = repo(vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])], &[])?;
    view.files.remove(&path("mise.toml")?);
    assert_eq!(
        detect(&view),
        Err(DetectError::MiseRequired {
            path: path("mise.toml")?,
        })
    );
    Ok(())
}

fn toolchain(
    channel: &str,
    components: &[&str],
    targets: &[&str],
    profile: Option<&str>,
) -> Toolchain {
    let owned = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect();
    Toolchain {
        channel: channel.to_owned(),
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
            "[toolchain]\ncomponents = [\"clippy\"]\n",
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
    assert_eq!(tools.config, path("mise.toml")?);
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
fn a_hidden_mise_configuration_is_read_when_mise_toml_is_absent() -> TestResult {
    let mut view = repo(
        vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])],
        &[(".mise.toml", "[tools]\nbun = \"1.4.4\"\n")],
    )?;
    view.files.remove(&path("mise.toml")?);
    let tools = detect(&view)?.tools;
    assert_eq!(tools.config, path(".mise.toml")?);
    assert_eq!(tools.tools.get("bun"), Some(&pin(&["1.4.4"], &[])));
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

#[test]
fn the_recorded_repository_sample_has_a_root_per_workspace_with_the_members_cargo_reports()
-> TestResult {
    let sample = samples().join("velnor-new");
    let facts = detect(&load(&sample)?)?;
    let mut reported = Vec::new();
    for entry in walk(&sample.join("cargo-metadata"))? {
        let json: serde_json::Value = serde_json::from_slice(&fs::read(&entry)?)?;
        let relative = entry
            .strip_prefix(sample.join("cargo-metadata"))?
            .to_str()
            .ok_or("path is not UTF-8")?
            .trim_end_matches(".json")
            .to_owned();
        let count = json["packages"].as_array().map_or(0, Vec::len);
        reported.push((relative, count));
    }
    reported.sort();
    let found: Vec<(String, usize)> = facts
        .rust_roots
        .iter()
        .map(|root| (root.manifest.to_string(), root.members.len()))
        .collect();
    let mut expected = reported;
    expected.sort();
    let mut found_sorted = found.clone();
    found_sorted.sort();
    assert_eq!(found_sorted, expected);
    assert_eq!(
        found
            .iter()
            .map(|(manifest, _)| manifest.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Cargo.toml",
            "ci-helper/Cargo.toml",
            "crates/velnor-runner/Cargo.toml",
            "generator/Cargo.toml",
            "local/Cargo.toml",
        ]
    );
    assert_eq!(
        facts.toolchain.map(|toolchain| toolchain.channel),
        Some("1.99.1".to_owned())
    );
    assert!(
        facts
            .tools
            .tools
            .contains_key("aqua:nextest-rs/nextest/cargo-nextest")
    );
    Ok(())
}

fn walk(directory: &Path) -> Result<Vec<std::path::PathBuf>, Box<dyn Error>> {
    let mut found = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            } else {
                found.push(entry.path());
            }
        }
    }
    Ok(found)
}
