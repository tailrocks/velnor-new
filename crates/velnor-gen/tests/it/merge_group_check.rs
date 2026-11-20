// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CargoCommand, FilterRef, LaneFeatures, PackageName, RepoPath, StepKind, TargetKind, TargetName,
};

#[test]
fn every_cargo_command_has_its_check_classification() {
    let package = || PackageName::from_static("app");
    let table = [
        (CargoCommand::FmtCheck, true),
        (CargoCommand::Clippy { extra: Vec::new() }, true),
        (CargoCommand::CheckAlone { package: package() }, true),
        (
            CargoCommand::BuildBinary {
                package: package(),
                binary: TargetName::from_static("app"),
            },
            false,
        ),
        (CargoCommand::BuildAll, false),
        (CargoCommand::NextestBuild, false),
        (
            CargoCommand::NextestRun {
                filter: FilterRef::All,
                ci_profile: true,
            },
            true,
        ),
        (
            CargoCommand::NextestFeatures {
                features: LaneFeatures::default(),
                packages: vec![package()],
                filter: FilterRef::All,
                ci_profile: true,
                reports: false,
            },
            true,
        ),
        (
            CargoCommand::ClippyFeatures {
                features: LaneFeatures::default(),
            },
            true,
        ),
        (
            CargoCommand::DocFeatures {
                features: LaneFeatures::default(),
            },
            true,
        ),
        (
            CargoCommand::LaneGuard {
                features: LaneFeatures::default(),
                packages: vec![package()],
                filter: String::new(),
                except: Vec::new(),
            },
            true,
        ),
        (
            CargoCommand::NextestArchive {
                file: RepoPath::from_static("target/nextest.tar.zst"),
            },
            false,
        ),
        (
            CargoCommand::NextestRunArchive {
                file: RepoPath::from_static("target/nextest.tar.zst"),
                filter: FilterRef::All,
                threads: None,
                ci_profile: true,
                no_tests: velnor_model::NoTests::Pass,
            },
            true,
        ),
        (
            CargoCommand::TestTarget {
                package: package(),
                target: TargetName::from_static("app"),
                kind: TargetKind::Test,
                features: Vec::new(),
            },
            true,
        ),
        (CargoCommand::Doctest, true),
        (CargoCommand::Doc, true),
        (
            CargoCommand::Deny {
                manifest: RepoPath::from_static("deny.toml"),
                config: None,
            },
            true,
        ),
        (CargoCommand::Machete, true),
    ];
    for (command, check) in table {
        assert_eq!(
            StepKind::Cargo(command.clone()).is_check(),
            check,
            "{command:?}"
        );
    }
}

#[test]
fn test_setup_and_the_advisory_fetch_are_checks_and_other_steps_are_not() {
    assert!(
        StepKind::TestSetup {
            name: String::from("setup"),
            run: String::from("true"),
        }
        .is_check()
    );
    // It serves the cargo-deny check (a check above), so it must skip with it.
    assert!(
        StepKind::AdvisoryFetch {
            configs: vec![None],
        }
        .is_check()
    );
    assert!(!StepKind::MergeQueueCheck.is_check());
}
