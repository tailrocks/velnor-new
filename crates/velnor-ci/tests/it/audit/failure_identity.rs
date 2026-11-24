// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;

use super::migration::{APP_SELECTION, build, package_output};
use super::selection::{NAMED, Run, prepared_project, wrapped_in};
use crate::io::Repo;

const LIB_BUILD: &[&str] = &[
    "cargo",
    "build",
    "-p",
    "lib",
    "--all-targets",
    "--offline",
    "--no-such-flag",
];

enum Expect {
    Named,
    Unnamed,
    UnnamedExplicitLib,
}

impl Expect {
    fn check(&self, case: &str, stderr: &str) {
        match self {
            Self::Named => assert!(stderr.contains(NAMED), "{case}: {stderr}"),
            Self::Unnamed => assert!(!stderr.contains(NAMED), "{case}: {stderr}"),
            Self::UnnamedExplicitLib => {
                assert!(!stderr.contains(NAMED), "{case}: {stderr}");
                assert!(
                    stderr.contains("explicit selection: lib"),
                    "{case}: {stderr}"
                );
            }
        }
    }
}

struct Case {
    name: &'static str,
    failing: &'static [&'static [&'static str]],
    failed_in: Run,
    refused_in: Run,
    expect: Expect,
}

const CASES: &[Case] = &[
    Case {
        name: "a_marker_from_a_run_is_ignored_when_the_current_invocation_has_no_run_identity",
        failing: &[LIB_BUILD],
        failed_in: Some(("1001", "1")),
        refused_in: None,
        expect: Expect::UnnamedExplicitLib,
    },
    Case {
        name: "a_marker_without_a_run_identity_is_named_by_an_invocation_without_one",
        failing: &[LIB_BUILD],
        failed_in: None,
        refused_in: None,
        expect: Expect::Named,
    },
    Case {
        name: "a_workspace_build_minus_another_package_is_named_for_a_package_it_compiled",
        failing: &[&[
            "cargo",
            "build",
            "--workspace",
            "--exclude",
            "app",
            "--all-targets",
            "--offline",
            "--no-such-flag",
        ]],
        failed_in: None,
        refused_in: None,
        expect: Expect::Named,
    },
    Case {
        name: "a_workspace_build_that_excludes_the_refused_package_is_not_named",
        failing: &[&[
            "cargo",
            "build",
            "--workspace",
            "--exclude",
            "lib",
            "--all-targets",
            "--offline",
            "--no-such-flag",
        ]],
        failed_in: None,
        refused_in: None,
        expect: Expect::UnnamedExplicitLib,
    },
    Case {
        name: "a_workspace_build_with_a_bare_exclusion_is_not_named",
        failing: &[&["cargo", "build", "--workspace", "--exclude"]],
        failed_in: None,
        refused_in: None,
        expect: Expect::Unnamed,
    },
    Case {
        name: "a_workspace_build_with_a_package_flag_and_no_name_is_not_named",
        failing: &[
            &["cargo", "build", "--workspace", "-p"],
            &["cargo", "build", "--workspace", "-p", "--offline"],
        ],
        failed_in: None,
        refused_in: None,
        expect: Expect::Unnamed,
    },
    Case {
        name: "a_workspace_build_with_a_target_filter_is_not_named",
        failing: &[&[
            "cargo",
            "build",
            "--workspace",
            "--exclude",
            "app",
            "--bins",
            "--offline",
            "--no-such-flag",
        ]],
        failed_in: None,
        refused_in: None,
        expect: Expect::Unnamed,
    },
    Case {
        name: "a_workspace_build_with_an_unprovable_exclusion_is_not_named",
        failing: &[&[
            "cargo",
            "build",
            "--workspace",
            "--exclude",
            "lib@0.1.0",
            "-p",
            "lib",
            "--all-targets",
            "--offline",
            "--no-such-flag",
        ]],
        failed_in: None,
        refused_in: None,
        expect: Expect::Unnamed,
    },
    Case {
        name: "a_workspace_build_that_excludes_and_names_the_refused_package_is_not_named",
        failing: &[&[
            "cargo",
            "build",
            "--workspace",
            "--exclude",
            "lib",
            "-p",
            "lib",
            "--all-targets",
            "--offline",
            "--no-such-flag",
        ]],
        failed_in: None,
        refused_in: None,
        expect: Expect::Unnamed,
    },
    Case {
        name: "a_workspace_build_with_a_package_id_spec_is_named",
        failing: &[&[
            "cargo",
            "build",
            "--workspace",
            "-p",
            "app@0.1.0",
            "--all-targets",
            "--offline",
            "--no-such-flag",
        ]],
        failed_in: None,
        refused_in: None,
        expect: Expect::Named,
    },
];

fn copy_into(from: &Path, to: &Path) {
    let output = Command::new("cp")
        .arg("-pR")
        .arg(from.join("."))
        .arg(to.join("."))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
}

fn pristine_copy(repo: &Repo) -> Repo {
    let copy = Repo::empty("failure-identity-pristine");
    copy_into(repo.root(), copy.root());
    copy
}

fn reset_to(repo: &Repo, pristine: &Repo) {
    for entry in fs::read_dir(repo.root()).unwrap_or_else(|e| unreachable!("{e}")) {
        let path = entry.unwrap_or_else(|e| unreachable!("{e}")).path();
        let is_dir = fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_dir());
        if is_dir {
            fs::remove_dir_all(&path).unwrap_or_else(|e| unreachable!("{e}"));
        } else {
            fs::remove_file(&path).unwrap_or_else(|e| unreachable!("{e}"));
        }
    }
    copy_into(pristine.root(), repo.root());
}

const WORKERS: usize = 4;

#[test]
fn the_failure_identity_cases_each_fail_for_their_own_reason() {
    let runs: Vec<(&Case, &[&str])> = CASES
        .iter()
        .flat_map(|case| case.failing.iter().map(move |failing| (case, *failing)))
        .collect();
    std::thread::scope(|scope| {
        for worker in 0..WORKERS {
            let runs = &runs;
            scope.spawn(move || {
                let repo = prepared_project();
                let pristine = pristine_copy(&repo);
                for (case, failing) in runs.iter().skip(worker).step_by(WORKERS) {
                    run_case(&repo, &pristine, case, failing);
                }
            });
        }
    });
}

fn run_case(repo: &Repo, pristine: &Repo, case: &Case, failing: &[&str]) {
    reset_to(repo, pristine);
    let baseline = build(repo, false);
    assert!(baseline.status.success(), "{}: {baseline:?}", case.name);
    let failed = wrapped_in(repo, failing, case.failed_in);
    assert!(!failed.status.success(), "{}: {failed:?}", case.name);
    let lib = package_output(repo, "lib");
    fs::remove_file(lib.with_extension("velnor-inputs.json"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let refused = wrapped_in(repo, &APP_SELECTION, case.refused_in);
    assert_eq!(refused.status.code(), Some(1), "{}: {refused:?}", case.name);
    case.expect
        .check(case.name, &String::from_utf8_lossy(&refused.stderr));
}
