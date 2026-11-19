// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use clap::{Arg, ArgAction, Command};

use crate::release_fixture::{Root, steps};
use crate::release_tagged_commit::render_with;
use crate::release_yml_binaries::job;
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const HYPHEN: Root = Root {
    dir: ".",
    package: "app",
    prefix: "-h",
    publishes: false,
};

fn words(run: &str) -> Vec<String> {
    let (mut all, mut word, mut quoted, mut open) = (Vec::new(), String::new(), false, false);
    for c in run.chars() {
        match c {
            '\'' => (quoted, open) = (!quoted, true),
            ' ' if !quoted => {
                if open {
                    all.push(std::mem::take(&mut word));
                    open = false;
                }
            }
            c => (word.push(c), open = true).0,
        }
    }
    if open {
        all.push(word);
    }
    all
}

fn pair(name: &'static str, hyphen: bool) -> Arg {
    Arg::new(name)
        .long(name)
        .num_args(2)
        .allow_hyphen_values(hyphen)
        .action(ArgAction::Append)
}

fn helper(hyphen: bool) -> Command {
    Command::new("check-config")
        .no_binary_name(true)
        .arg(Arg::new("files").num_args(1..).required(true))
        .arg(pair("tag-pattern", hyphen))
        .arg(pair("branch-prefix", hyphen))
        .arg(Arg::new("repo").long("repo"))
        .arg(
            Arg::new("manifests")
                .long("manifests")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("released-versions")
                .long("released-versions")
                .action(ArgAction::SetTrue),
        )
}

fn rendered_args() -> Result<Vec<String>, Box<dyn Error>> {
    let tree = render_with("0.1.81", &[HYPHEN], "[[binaries]]\npackage = \"app\"\n")?;
    let run = steps(job(&tree, "tag")?)
        .iter()
        .filter_map(|step| step.get("run").and_then(Node::text))
        .find(|run| run.starts_with("velnor-ci release check-config"))
        .ok_or("no check-config step")?;
    Ok(words(run).split_off(3))
}

#[test]
fn a_hyphen_prefix_renders_as_a_value_the_helper_parser_accepts() -> TestResult {
    let args = rendered_args()?;
    assert!(
        args.windows(3)
            .any(|w| w == ["--branch-prefix", "release-plz.toml", "-h"]),
        "{args:?}"
    );
    let matches = helper(true).try_get_matches_from(&args)?;
    let prefixes: Vec<&String> = matches
        .get_many("branch-prefix")
        .ok_or("no branch prefix")?
        .collect();
    assert_eq!(prefixes, ["release-plz.toml", "-h"]);
    assert!(matches.get_flag("manifests"));
    Ok(())
}

#[test]
fn the_same_rendering_is_read_as_help_by_a_parser_that_forbids_hyphen_values() -> TestResult {
    let error = helper(false)
        .try_get_matches_from(rendered_args()?)
        .err()
        .ok_or("parsed")?;
    assert_eq!(error.kind(), clap::error::ErrorKind::WrongNumberOfValues);
    Ok(())
}
