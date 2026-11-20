// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

#[cfg(not(target_os = "linux"))]
use crate::prepare::OsRelease;
use crate::prepare::{Setup, digest, prepare, reported, scratch_root};

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
fn compat_follows_the_distro_in_os_release() {
    let release = |name: &str, body: &str| {
        let dir = scratch_root("os-release");
        fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
        let path = dir.join(name);
        fs::write(&path, body).unwrap_or_else(|e| unreachable!("{e}"));
        let path: &'static str = Box::leak(path.display().to_string().into_boxed_str());
        compat_with("compat-os-release", |setup| {
            setup.env.push(("VELNOR_OS_RELEASE", path));
        })
    };
    let base = release("a", "ID=ubuntu\nVERSION_ID=\"24.04\"\n");
    assert_eq!(base, release("b", "ID=ubuntu\nVERSION_ID=\"24.04\"\n"));
    assert_ne!(base, release("c", "ID=ubuntu\nVERSION_ID=\"22.04\"\n"));
    assert_ne!(base, release("d", "ID=debian\nVERSION_ID=\"24.04\"\n"));
}

#[test]
fn an_unreadable_os_release_degrades_to_a_cold_run_with_a_warning() {
    let missing = scratch_root("os-release-missing").join("no-such-os-release");
    let missing: &'static str = Box::leak(missing.display().to_string().into_boxed_str());
    let cold = || {
        let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
        setup.env.push(("ImageOS", "ubuntu24"));
        setup.env.push(("VELNOR_OS_RELEASE", missing));
        prepare("compat-no-os-release", &setup)
    };
    let (first, second) = (cold(), cold());
    assert!(first.output.status.success(), "{:?}", first.output);
    let stdout = String::from_utf8_lossy(&first.output.stdout);
    assert!(
        stdout.contains("::warning::") && stdout.contains("os-release"),
        "{stdout}"
    );
    assert_ne!(digest(&first), "");
    assert_ne!(
        digest(&first),
        digest(&second),
        "a cold run must share no key"
    );
    let readable = compat_with("compat-no-os-release", |setup| {
        setup.env.push(("ImageOS", "ubuntu24"));
    });
    assert_ne!(digest(&first), readable);
}

#[cfg(not(target_os = "linux"))]
#[test]
fn a_platform_without_os_release_still_prepares_with_its_own_identity() {
    let mut setup = Setup::new(reported("1.99.1", Some("0.9.151")));
    setup.os_release = OsRelease::Platform;
    let run = prepare("compat-no-os-release-platform", &setup);
    assert!(run.output.status.success(), "{:?}", run.output);
    assert_ne!(digest(&run), "");
}
