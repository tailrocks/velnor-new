// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{
    CargoCommand, CustomHarness, FilterRef, PackageName, RepoFacts, RootSlug, RustRoot, TargetKind,
};

use crate::doc_steps::has_doctest_library;
use crate::harness::{runs_in_direct_step, selector_id, uses_nextest};
use crate::lane::Lane;

pub(crate) struct Member {
    pub(crate) root: RustRoot,
    pub(crate) lane: Lane,
    pub(crate) nextest: bool,
    pub(crate) doctests: bool,
}

impl Member {
    pub(crate) fn new(root: &RustRoot, lane: &Lane, facts: &RepoFacts, grouped: bool) -> Self {
        Self {
            root: root.clone(),
            lane: lane.with_slug(RootSlug::of(&root.manifest), grouped),
            nextest: root
                .members
                .iter()
                .any(|member| uses_nextest(&facts.config, member)),
            doctests: has_doctest_library(root),
        }
    }

    pub(crate) fn direct_harnesses(&self, facts: &RepoFacts) -> Vec<CargoCommand> {
        let mut seen = BTreeSet::new();
        self.root
            .members
            .iter()
            .flat_map(|entry| {
                entry
                    .custom_harnesses
                    .iter()
                    .filter(|harness| runs_in_direct_step(&facts.config, entry, harness))
                    .filter(|harness| seen.insert((entry.name.clone(), selector_id(harness))))
                    .map(|harness| CargoCommand::TestTarget {
                        package: entry.name.clone(),
                        target: harness.name.clone(),
                        kind: harness.kind,
                        features: harness.required_features.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub(crate) fn direct_binaries(&self, facts: &RepoFacts) -> Vec<String> {
        self.root
            .members
            .iter()
            .flat_map(|entry| {
                entry
                    .custom_harnesses
                    .iter()
                    .filter(|harness| runs_in_direct_step(&facts.config, entry, harness))
                    .map(|harness| binary_id(&entry.name, harness))
            })
            .collect()
    }

    pub(crate) fn filter(&self, facts: &RepoFacts) -> FilterRef {
        let direct = self.direct_binaries(facts);
        if direct.is_empty() {
            FilterRef::All
        } else {
            FilterRef::AllExcept(direct)
        }
    }

    pub(crate) fn tests(&self, facts: &RepoFacts) -> Vec<CargoCommand> {
        self.nextest_run(facts)
            .into_iter()
            .chain(self.built_without_nextest(facts))
            .collect()
    }

    pub(crate) fn nextest_run(&self, facts: &RepoFacts) -> Option<CargoCommand> {
        self.nextest.then(|| CargoCommand::NextestRun {
            filter: self.filter(facts),
            ci_profile: self.root.nextest_ci_profile,
        })
    }

    pub(crate) fn feature_lanes(&self, facts: &RepoFacts) -> Vec<CargoCommand> {
        let direct = match self.filter(facts) {
            FilterRef::AllExcept(binaries) => binaries,
            _ => Vec::new(),
        };
        crate::config_test_lane::commands(&facts.config, &self.root, &direct)
    }

    pub(crate) fn built_without_nextest(&self, facts: &RepoFacts) -> Vec<CargoCommand> {
        self.feature_lanes(facts)
            .into_iter()
            .chain(self.direct_harnesses(facts))
            .chain(self.doctests())
            .collect()
    }

    pub(crate) fn doctests(&self) -> Vec<CargoCommand> {
        self.doctests
            .then_some(CargoCommand::Doctest)
            .into_iter()
            .collect()
    }
}

pub(crate) fn lint() -> Vec<CargoCommand> {
    vec![
        CargoCommand::FmtCheck,
        CargoCommand::Clippy { extra: Vec::new() },
    ]
}

pub(crate) fn binary_id(package: &PackageName, harness: &CustomHarness) -> String {
    let name = &harness.name;
    match harness.kind {
        TargetKind::Lib | TargetKind::ProcMacro => package.to_string(),
        TargetKind::Bin => format!("{package}::bin/{name}"),
        TargetKind::Example => format!("{package}::example/{name}"),
        TargetKind::Bench => format!("{package}::bench/{name}"),
        TargetKind::Test | TargetKind::BuildScript => format!("{package}::{name}"),
    }
}

pub(crate) fn check_alone(root: &RustRoot) -> Vec<CargoCommand> {
    root.members
        .iter()
        .filter(|member| {
            member.targets.iter().any(|target| {
                !target.requires_features
                    && target.kinds.iter().any(|kind| {
                        matches!(
                            kind,
                            TargetKind::Lib | TargetKind::ProcMacro | TargetKind::Bin
                        )
                    })
            })
        })
        .map(|member| CargoCommand::CheckAlone {
            package: member.name.clone(),
        })
        .collect()
}
