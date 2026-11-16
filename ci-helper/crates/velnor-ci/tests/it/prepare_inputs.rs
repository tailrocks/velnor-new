// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::prepare::{MISE, Setup, digest, prepare, reported};

fn compat_with(name: &str, change: impl FnOnce(&mut Setup)) -> String {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    change(&mut setup);
    let run = prepare(name, &setup);
    assert!(run.output.status.success(), "{:?}", run.output);
    digest(&run).to_owned()
}

#[test]
fn compat_follows_a_repository_dependency_profile_override() {
    let none = compat_with("compat-inputs", |_| {});
    let debug = compat_with("compat-inputs", |setup| {
        setup.repo_cargo_config = Some("[profile.dev.package.\"*\"]\ndebug = true\n");
    });
    let off = compat_with("compat-inputs", |setup| {
        setup.repo_cargo_config = Some("[profile.dev.package.\"*\"]\ndebug = false\n");
    });
    let unrelated = compat_with("compat-inputs", |setup| {
        setup.repo_cargo_config = Some("[build]\njobs = 2\n");
    });
    assert_ne!(none, debug);
    assert_ne!(debug, off);
    assert_eq!(none, unrelated);
}

#[test]
fn compat_follows_a_gitignored_repository_profile_override() {
    let none = compat_with("compat-ignored", |_| {});
    let ignored = compat_with("compat-ignored", |setup| {
        setup.repo_cargo_config = Some("[profile.dev.package.\"*\"]\ndebug = true\n");
        setup.ignored = true;
    });
    assert_ne!(none, ignored);
}

#[test]
fn compat_reads_the_repository_profile_without_asking_git() {
    let with_git = compat_with("compat-plain", |setup| {
        setup.repo_cargo_config = Some("[profile.dev.package.\"*\"]\ndebug = true\n");
    });
    let without_git = compat_with("compat-plain", |setup| {
        setup.repo_cargo_config = Some("[profile.dev.package.\"*\"]\ndebug = true\n");
        setup.plain = true;
    });
    assert_eq!(with_git, without_git);
}

#[test]
fn compat_follows_the_runner_image() {
    let image = |value: Option<&'static str>| {
        compat_with("compat-inputs", |setup| {
            if let Some(value) = value {
                setup.env.push(("ImageOS", value));
            }
        })
    };
    let (unset, new, old) = (
        image(None),
        image(Some("ubuntu24")),
        image(Some("ubuntu22")),
    );
    assert_ne!(new, old);
    assert_ne!(new, unset);
    assert_eq!(new, image(Some("ubuntu24")));
}

#[test]
fn rust_before_1_71_gets_numeric_debug_values() {
    let mut setup = Setup::new(reported("1.70.0", Some("0.9.151")));
    setup.release = "1.70.0";
    setup.mise_toml = MISE.replace("1.99.1", "1.70.0");
    let run = prepare("old-rust", &setup);
    assert!(run.output.status.success(), "{:?}", run.output);
    assert!(
        run.env_file.contains("CARGO_PROFILE_DEV_DEBUG=1\n"),
        "{}",
        run.env_file
    );
    assert!(run.env_file.contains("CARGO_PROFILE_TEST_DEBUG=1\n"));
    assert!(
        run.velnor_config.contains("debug = 0\n"),
        "{}",
        run.velnor_config
    );
    assert!(!run.velnor_config.contains("\"none\""));
}
