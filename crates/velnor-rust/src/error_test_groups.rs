// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use crate::error::Reason;

impl Reason {
    pub(crate) fn fmt_test_groups(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTestGroupRoot(root) => write!(
                f,
                "test_groups names the root {root}, which is not a Cargo root of this repository; name the folder of a root's Cargo.toml, or . for the repository root"
            ),
            Self::TestGroupWithoutNextest(name) => write!(
                f,
                "test group {name} belongs to a root with no nextest tests; a group runs a nextest filter from the root's archive"
            ),
            Self::PartitionsWithoutNextest(name) => write!(
                f,
                "test_groups.partitions is set on group {name}, whose root has no nextest tests; partitions are a nextest feature (--partition), so remove partitions or give the root tests that nextest runs"
            ),
            Self::TestGroupArtifact { key, value, fix } => write!(f, "{key} {value:?} is refused: {fix}"),
            Self::ArtifactNameTaken(name) => write!(
                f,
                "test_groups.artifacts.name {name:?} is used twice; an artifact name is unique in a workflow run, and a partitioned group appends -<partition> to each name, so give each artifact its own name"
            ),
            Self::TestGroupFilter(filter) => write!(
                f,
                "test_groups.filter {filter:?} is empty or has a character a generated one-command step cannot carry (a control character, a quote, $, a backtick, a backslash, |, &, ; or <); write nextest's and, or and not instead of &, | and !"
            ),
            Self::TestGroupsWithPrivateSources => f.write_str(
                "[[test_groups]] and [private_sources] cannot both be set: a test group runs from the nextest archive, and that archive holds the compiled private dependencies, which anyone who can read the repository's workflow artifacts could download; remove the test groups, or remove [private_sources] when every dependency is public",
            ),
            Self::UnknownTestSetupRoot(root) => write!(
                f,
                "test_setup names the root {root}, which is not a Cargo root of this repository; name the folder of a root's Cargo.toml, or . for the repository root"
            ),
            _ => Ok(()),
        }
    }
}
