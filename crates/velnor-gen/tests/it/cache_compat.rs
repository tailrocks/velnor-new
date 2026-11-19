// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CiDebugInfo, RepoPath};
use velnor_rust::{
    CONFIG_PATH, CompatInputs, DetectError, Reason, build_profile, ci_debuginfo, compat, detect,
    repo_profile, rustc_release,
};

use crate::facts::{TestResult, lib, repo};

const MODERN: (u32, u32) = (1, 99);

type Mutation = (&'static str, fn(&mut CompatInputs));

fn inputs() -> CompatInputs {
    CompatInputs {
        rustc_verbose: "rustc 1.99.1\nhost: aarch64-apple-darwin\n".to_owned(),
        target: None,
        profile: build_profile(CiDebugInfo::LineTablesOnly, MODERN),
        repo_profile: String::new(),
        image: "ubuntu24".to_owned(),
        rustflags: String::new(),
        workspace: "/home/runner/work/p/p".to_owned(),
        driver: "cargo".to_owned(),
    }
}

#[test]
fn changing_any_compat_input_changes_the_digest() {
    let base = compat(&inputs());
    assert_eq!(base, compat(&inputs()));
    assert_eq!(base.len(), 16);
    let mutations: [Mutation; 12] = [
        ("rustc", |i| i.rustc_verbose.push_str("release: 1.99.2\n")),
        ("target", |i| {
            i.target = Some("x86_64-unknown-linux-gnu".to_owned());
        }),
        ("debuginfo full", |i| {
            i.profile = build_profile(CiDebugInfo::Full, MODERN);
        }),
        ("debuginfo none", |i| {
            i.profile = build_profile(CiDebugInfo::None, MODERN);
        }),
        ("rustflags", |i| {
            i.rustflags = "-C target-cpu=native".to_owned();
        }),
        ("workspace", |i| i.workspace = "/work".to_owned()),
        ("driver", |i| i.driver = "mbx".to_owned()),
        ("env", |i| i.profile.env.push(("EXTRA", "1"))),
        ("config", |i| i.profile.cargo_config = "[build]\n"),
        ("repo profile", |i| {
            i.repo_profile =
                ".cargo/config.toml\n[profile.dev.package.\"*\"]\ndebug = true\n".to_owned();
        }),
        ("image", |i| i.image = "ubuntu22".to_owned()),
        ("old rustc", |i| {
            i.profile = build_profile(CiDebugInfo::LineTablesOnly, (1, 70));
        }),
    ];
    for (name, mutate) in mutations {
        let mut changed = inputs();
        mutate(&mut changed);
        assert_ne!(base, compat(&changed), "{name}");
    }
    let (mut left, mut right) = (inputs(), inputs());
    left.rustflags = "a".to_owned();
    left.workspace = "b".to_owned();
    right.rustflags = "ab".to_owned();
    right.workspace = String::new();
    assert_ne!(compat(&left), compat(&right));
    let (mut left, mut right) = (inputs(), inputs());
    left.rustflags = "x\nworkspace:y".to_owned();
    left.workspace = "z".to_owned();
    right.rustflags = "x".to_owned();
    right.workspace = "y\nworkspace:z".to_owned();
    assert_ne!(compat(&left), compat(&right));
}

#[test]
fn the_ci_debuginfo_key_picks_the_profile_and_rejects_other_values() -> TestResult {
    let files =
        |text: &str| std::collections::BTreeMap::from([(CONFIG_PATH, text.as_bytes().to_vec())]);
    assert_eq!(
        ci_debuginfo(&std::collections::BTreeMap::new()),
        Ok(CiDebugInfo::LineTablesOnly)
    );
    for (value, expected, workspace) in [
        ("full", CiDebugInfo::Full, "full"),
        (
            "line-tables-only",
            CiDebugInfo::LineTablesOnly,
            "line-tables-only",
        ),
        ("none", CiDebugInfo::None, "none"),
    ] {
        let found = ci_debuginfo(&files(&format!("ci_debuginfo = \"{value}\"\n")))?;
        assert_eq!(found, expected);
        let profile = build_profile(found, MODERN);
        assert!(
            profile
                .env
                .contains(&("CARGO_PROFILE_DEV_DEBUG", workspace))
        );
        assert!(
            profile
                .env
                .contains(&("CARGO_PROFILE_TEST_DEBUG", workspace))
        );
        assert_eq!(
            profile.cargo_config.is_empty(),
            expected == CiDebugInfo::Full
        );
    }
    assert_eq!(
        ci_debuginfo(&files("ci_debuginfo = \"some\"\n")),
        Err(DetectError::Invalid {
            path: CONFIG_PATH,
            reason: Reason::InvalidValue {
                key: "ci_debuginfo",
                value: "some".to_owned(),
            },
        })
    );
    let mut view = repo(vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])], &[])?;
    view.tracked_paths.insert(CONFIG_PATH);
    view.files
        .insert(CONFIG_PATH, b"ci_debuginfo = \"none\"\n".to_vec());
    assert_eq!(detect(&view)?.config.ci_debuginfo, CiDebugInfo::None);
    Ok(())
}

#[test]
fn rust_before_1_71_gets_numeric_debug_values_and_later_releases_keep_the_strings() {
    let numeric = |debuginfo, rustc| {
        let profile = build_profile(debuginfo, rustc);
        (
            profile
                .env
                .iter()
                .find(|(name, _)| *name == "CARGO_PROFILE_DEV_DEBUG")
                .map(|(_, value)| *value),
            profile.cargo_config.contains("debug = 0"),
        )
    };
    for (debuginfo, old, config) in [
        (CiDebugInfo::Full, "2", false),
        (CiDebugInfo::LineTablesOnly, "1", true),
        (CiDebugInfo::None, "0", true),
    ] {
        assert_eq!(numeric(debuginfo, (1, 70)), (Some(old), config));
        assert_eq!(numeric(debuginfo, (0, 99)), (Some(old), config));
    }
    assert_eq!(
        numeric(CiDebugInfo::LineTablesOnly, (1, 71)),
        (Some("line-tables-only"), false)
    );
    assert_eq!(numeric(CiDebugInfo::None, (2, 0)), (Some("none"), false));
    assert!(
        !build_profile(CiDebugInfo::None, (1, 71))
            .cargo_config
            .contains("debug = 0")
    );
}

#[test]
fn the_rustc_release_is_read_from_the_verbose_report() {
    for (text, expected) in [
        ("rustc 1.70.1\nrelease: 1.70.1\n", Some((1, 70))),
        ("release: 1.71.0-nightly\n", Some((1, 71))),
        ("release: 1.72.0-beta.3\n", Some((1, 72))),
        ("release: nightly\n", None),
        ("rustc 1.70.1\n", None),
    ] {
        assert_eq!(rustc_release(text), expected, "{text}");
    }
}

#[test]
fn a_repository_dependency_profile_is_read_from_the_cargo_configs_that_cargo_loads() -> TestResult {
    let files = |entries: &[(&str, &str)]| -> Result<_, Box<dyn std::error::Error>> {
        entries
            .iter()
            .map(|(path, text)| Ok((RepoPath::new(*path)?, text.as_bytes().to_vec())))
            .collect()
    };
    let profile = "[profile.dev.package.\"*\"]\ndebug = true\n";
    assert_eq!(repo_profile(&files(&[])?)?, "");
    assert_eq!(
        repo_profile(&files(&[(".cargo/config.toml", "[build]\njobs = 2\n")])?)?,
        ""
    );
    let toml = repo_profile(&files(&[(".cargo/config.toml", profile)])?)?;
    let bare = repo_profile(&files(&[(".cargo/config", profile)])?)?;
    assert!(toml.contains("debug = true") && toml.starts_with(".cargo/config.toml\n"));
    assert!(bare.starts_with(".cargo/config\n"));
    let other = repo_profile(&files(&[(
        ".cargo/config.toml",
        "[profile.dev.package.\"*\"]\ndebug = false\n",
    )])?)?;
    assert_ne!(toml, other);
    let both = repo_profile(&files(&[
        (".cargo/config.toml", profile),
        (".cargo/config", "[build]\njobs = 2\n"),
    ])?)?;
    assert_eq!(both, "");
    let reformatted = repo_profile(&files(&[(
        ".cargo/config.toml",
        "[profile.dev.package.\"*\"]\n\n\n  debug   =  true # why\n",
    )])?)?;
    assert_eq!(toml, reformatted);
    let broken = repo_profile(&files(&[("lab/.cargo/config.toml", "[profile\n")])?);
    assert!(matches!(
        broken,
        Err(DetectError::Invalid { ref path, .. }) if path.as_str() == "lab/.cargo/config.toml"
    ));
    Ok(())
}
