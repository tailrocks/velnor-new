// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    BuildRecord, CargoCommand, CommitSha, DenyChecks, FilterRef, HelperRelease, HelperVersion, Job,
    JobId, PackageName, Permissions, Platform, PolicyTools, PrRun, ReleaseAssetPart, RunnerLabel,
    RustCiConfig, Sha256, StepKind, TargetName, ToolPins, When,
};

use super::*;

fn job(id: JobId, kinds: Vec<StepKind>) -> Job {
    Job {
        id,
        name: None,
        comment: None,
        needs: Vec::new(),
        when: When::Success,
        pr: PrRun::Always,
        runs_on: RunnerLabel::from_static("ubuntu-latest"),
        permissions: Permissions(BTreeMap::new()),
        timeout_minutes: 30,
        gate: None,
        skip_after_merge_group: None,
        plan_outputs: Vec::new(),
        group_outputs: Vec::new(),
        reports_ignored: false,
        build_record: BuildRecord::Unrecorded,
        chained_saves: false,
        mbx_stats: velnor_model::MbxStats::Summary,
        selection_name: None,
        steps: kinds.into_iter().map(step).collect(),
    }
}

#[test]
fn the_checks_and_only_the_checks_are_classified_as_checks() {
    let checks = [
        CargoCommand::FmtCheck,
        CargoCommand::Clippy { extra: Vec::new() },
        CargoCommand::Doc,
        CargoCommand::Doctest,
        CargoCommand::Machete,
        CargoCommand::NextestRun {
            filter: FilterRef::All,
            ci_profile: true,
        },
    ];
    let builds = [
        CargoCommand::BuildAll,
        CargoCommand::NextestBuild,
        CargoCommand::BuildBinary {
            package: PackageName::from_static("app"),
            binary: TargetName::from_static("app"),
        },
    ];
    for command in checks {
        assert!(!command.is_target_build(), "{command:?}");
    }
    for command in builds {
        assert!(command.is_target_build(), "{command:?}");
    }
}

#[test]
#[should_panic(expected = "reads no merge-group-passed step")]
fn a_skip_naming_a_missing_merge_group_step_stops_generation() {
    let mut marked = step(StepKind::Cargo(CargoCommand::FmtCheck));
    marked.skip_after_merge_group = Some(PlanFrom::ThisJob);
    let mut jobs = [job(RUST_JOB, vec![StepKind::Cargo(CargoCommand::Doc)])];
    jobs[0].steps.push(marked);
    assert_skips_resolve(&jobs);
}

fn build_inputs(build: bool) -> Job {
    let mut kinds = vec![
        StepKind::ReleaseAssets {
            assets: Vec::new(),
            part: ReleaseAssetPart::Check,
        },
        StepKind::TestSetup {
            name: String::from("setup"),
            run: String::from("true"),
            before_build: true,
        },
    ];
    if build {
        kinds.push(StepKind::Cargo(CargoCommand::NextestBuild));
    }
    kinds.push(StepKind::Cargo(CargoCommand::Clippy { extra: Vec::new() }));
    job(RUST_JOB, kinds)
}

#[test]
fn the_build_inputs_are_skipped_only_when_no_target_build_of_the_job_survives() {
    for (build, skipped) in [(true, false), (false, true)] {
        let mut jobs = [build_inputs(build)];
        mark(&mut jobs, true, false, None, &|_| false);
        let marks: Vec<bool> = jobs[0].steps[..2]
            .iter()
            .map(|step| step.skip_after_merge_group.is_some())
            .collect();
        assert_eq!(marks, [skipped, skipped], "build: {build}");
        assert!(
            jobs[0]
                .steps
                .last()
                .is_some_and(|step| step.skip_after_merge_group.is_some()),
            "the clippy check is skipped either way"
        );
    }
}

fn root(manifest: &'static str, depends_on: &[&'static str]) -> RustRoot {
    RustRoot {
        manifest: RepoPath::from_static(manifest),
        target_dir: RepoPath::from_static("target"),
        nextest_ci_profile: false,
        members: Vec::new(),
        depends_on: depends_on
            .iter()
            .copied()
            .map(RepoPath::from_static)
            .collect(),
        external_dependencies: Vec::new(),
    }
}

fn at(kind: StepKind, directory: &'static str, manifest: Option<&'static str>) -> Step {
    Step {
        directory: Some(RepoPath::from_static(directory)),
        manifest: manifest.map(RepoPath::from_static),
        ..step(kind)
    }
}

fn sibling_job(build_dir: &'static str, build_manifest: &'static str) -> Job {
    let setup = StepKind::TestSetup {
        name: String::from("setup"),
        run: String::from("true"),
        before_build: true,
    };
    let mut job = job(RUST_JOB, Vec::new());
    job.steps = vec![
        at(setup, "tools/a", None),
        at(
            StepKind::Cargo(CargoCommand::Clippy { extra: Vec::new() }),
            "tools/a",
            Some("tools/a/Cargo.toml"),
        ),
        at(
            StepKind::Cargo(CargoCommand::NextestBuild),
            build_dir,
            Some(build_manifest),
        ),
    ];
    job
}

#[test]
fn a_setup_of_one_root_stays_only_while_a_surviving_build_compiles_its_root() {
    let cases: [(&[&str], bool); 2] = [(&[], false), (&["tools/a/Cargo.toml"], true)];
    for (depends_on, kept) in cases {
        let roots = [
            root("tools/a/Cargo.toml", &[]),
            root("tools/b/Cargo.toml", depends_on),
        ];
        let mut jobs = [sibling_job("tools/b", "tools/b/Cargo.toml")];
        mark(&mut jobs, true, false, Some(&roots), &|_| false);
        assert_eq!(
            jobs[0].steps[0].skip_after_merge_group.is_none(),
            kept,
            "tools/b depends on {depends_on:?}"
        );
    }
}

fn facts_at(helper: &'static str, rust_roots: Vec<RustRoot>) -> RepoFacts {
    let release = HelperRelease::new(
        HelperVersion::from_static(helper),
        CommitSha::from_static("0123456789abcdef0123456789abcdef01234567"),
        BTreeMap::from([(
            Platform::LinuxX64,
            Sha256::from_static("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
        )]),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    RepoFacts {
        repository: String::new(),
        rust_roots,
        toolchain: None,
        tools: ToolPins {
            configs: Vec::new(),
            locked_file: true,
            tools: BTreeMap::new(),
        },
        config: RustCiConfig::default(),
        helper: Some(release),
        compile_inputs: BTreeMap::new(),
        inputs_lock_identified: false,
        timings: BTreeMap::new(),
        timings_notes: Vec::new(),
        private_sources: None,
        private_base_caches: false,
        private_suggestion: None,
        unowned: BTreeMap::new(),
        policy: PolicyTools {
            deny: false,
            machete: false,
            deny_off: false,
            checks: DenyChecks::of(&[]),
        },
        deny_configs: BTreeMap::new(),
        weakenings: Vec::new(),
    }
}

#[test]
fn the_setup_scope_follows_the_helper_pin_from_0_1_112() {
    for (helper, kept) in [("0.1.109", true), ("0.1.111", true), ("0.1.112", false)] {
        let roots = vec![
            root("tools/a/Cargo.toml", &[]),
            root("tools/b/Cargo.toml", &[]),
        ];
        let facts = facts_at(helper, roots);
        let mut jobs = [sibling_job("tools/b", "tools/b/Cargo.toml")];
        apply(&facts, &mut jobs, true, &|_| false);
        assert_eq!(
            jobs[0].steps[0].skip_after_merge_group.is_none(),
            kept,
            "helper {helper}"
        );
    }
}
