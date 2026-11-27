// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{PackageName, RepoPath, RepoView, TargetKind};

use crate::doc_steps::tokens::{Token, tokens};

const PREFIXES: &[&str] = &[
    "cargo:rerun-if-changed",
    "cargo::rerun-if-changed",
    "cargo:rerun-if-env-changed",
    "cargo::rerun-if-env-changed",
];
const PRINTING: &[&str] = &["println", "print", "write", "writeln"];
const LINE_TO_ADD: &str = "println!(\"cargo:rerun-if-changed=build.rs\");";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SilentBuildScript {
    pub package: PackageName,
    pub source: RepoPath,
}

impl SilentBuildScript {
    #[must_use]
    pub fn note(&self) -> String {
        format!(
            "{} of package {} declares no cargo:rerun-if-changed and no cargo:rerun-if-env-changed, so cargo rescans the whole package on every run and rebuilds the package after any file in it changes; add `{LINE_TO_ADD}` to its main function, and one line for each other file it reads",
            self.source, self.package
        )
    }
}

#[must_use]
pub fn build_script_sources(view: &RepoView) -> Vec<RepoPath> {
    let mut sources: Vec<RepoPath> = view
        .rust_metadata
        .values()
        .flat_map(|metadata| &metadata.members)
        .flat_map(|member| &member.targets)
        .filter(|target| target.kinds.contains(&TargetKind::BuildScript))
        .filter_map(|target| target.source.clone())
        .collect();
    sources.sort();
    sources.dedup();
    sources
}

#[must_use]
pub fn silent_build_scripts(view: &RepoView) -> Vec<SilentBuildScript> {
    let mut found: Vec<SilentBuildScript> = view
        .rust_metadata
        .values()
        .flat_map(|metadata| &metadata.members)
        .flat_map(|member| {
            member
                .targets
                .iter()
                .filter(|target| target.kinds.contains(&TargetKind::BuildScript))
                .filter_map(|target| target.source.clone())
                .map(|source| SilentBuildScript {
                    package: member.name.clone(),
                    source,
                })
        })
        .filter(|script| {
            view.files
                .get(&script.source)
                .is_some_and(|bytes| !emits_rerun_directive(&String::from_utf8_lossy(bytes)))
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

fn emits_rerun_directive(source: &str) -> bool {
    let found = tokens(source);
    let mut at = 0;
    while at < found.len() {
        let printing = matches!(found[at], Token::Ident(name) if PRINTING.contains(&name))
            && found.get(at + 1) == Some(&Token::Punct('!'));
        if printing {
            let mut depth = 0_usize;
            for token in &found[at + 2..] {
                match token {
                    Token::Punct('(' | '[' | '{') => depth += 1,
                    Token::Punct(')' | ']' | '}') => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            break;
                        }
                    }
                    Token::Str(text)
                        if depth > 0 && PREFIXES.iter().any(|p| text.starts_with(p)) =>
                    {
                        return true;
                    }
                    _ => {}
                }
            }
        }
        at += 1;
    }
    false
}
