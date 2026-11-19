// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "velnor-gen",
    about = "Generate the Rust CI workflow of a repository"
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        default_value = ".",
        help = "A path inside the repository"
    )]
    pub root: PathBuf,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Command {
    #[command(about = "Write the generated files and remove the ones no longer generated")]
    Generate,
    #[command(about = "Exit 1 when a generated file differs from what generate would write")]
    Check,
    #[command(
        about = "Download SHA256SUMS and COMMIT of a release and write .velnor/rust-ci.lock for it"
    )]
    PinHelper {
        #[arg(help = "The release version, MAJOR.MINOR.PATCH")]
        version: String,
    },
    #[command(
        about = "Print the [[test_groups]] that would shorten the wait, from .velnor/rust-ci-timings.json: one for each test binary that takes more than a third of its root's test time; writes nothing"
    )]
    SuggestGroups,
    #[command(
        about = "Exit 1 unless the tag is rust-ci-v or tools-v followed by the version of this program"
    )]
    CheckTag {
        #[arg(help = "The release tag")]
        tag: String,
    },
    #[command(subcommand, about = "Commands about release.yml")]
    Release(ReleaseCommand),
    #[command(about = "Record a golden-test sample from a real checkout into generator/samples")]
    RecordSample {
        #[arg(help = "A path inside the checkout to record")]
        checkout: PathBuf,
        #[arg(help = "The sample name: lowercase letters, digits and dashes")]
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum ReleaseCommand {
    #[command(
        about = "Print the two tag rulesets that let only the velnor-release App create the release tags, from the tag patterns in the release-plz.toml files, as one JSON object per line; create each with one request: velnor-gen release ruleset | while IFS= read -r ruleset; do printf '%s' \"$ruleset\" | gh api --method POST repos/<owner>/<repo>/rulesets --input - || exit 1; done; then check both with release ruleset-drift"
    )]
    Ruleset,
    #[command(
        about = "Compare the live tag rulesets, a JSON array of the full ruleset objects that an admin read of repos/<owner>/<repo>/rulesets/<id> returns, with the generated ones, bypass actors included; exits 1 on a difference"
    )]
    RulesetDrift {
        #[arg(help = "The file with the live rulesets")]
        live: PathBuf,
    },
}
