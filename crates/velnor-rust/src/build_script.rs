// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{PackageName, RepoPath, RepoView, RustMember, TargetKind};

use crate::build_modules::reach;
use crate::doc_steps::tokens::{Token, tokens};
use crate::macro_names::group_end;

const PREFIXES: &[&str] = &[
    "cargo:rerun-if-changed",
    "cargo::rerun-if-changed",
    "cargo:rerun-if-env-changed",
    "cargo::rerun-if-env-changed",
];
const CHANGED: &[&str] = &["cargo:rerun-if-changed", "cargo::rerun-if-changed"];
const RESERVED: &[&str] = &["CARGO_MANIFEST_DIR", "OUT_DIR"];
const SET_ENV: &str = "rustc-env=";
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Declared {
    pub(crate) tracks_changes: bool,
    pub(crate) overrides_environment: bool,
}

#[must_use]
pub(crate) fn declared(view: &RepoView, member: &RustMember) -> Declared {
    let mut found = Declared::default();
    for source in member
        .targets
        .iter()
        .filter(|target| target.kinds.contains(&TargetKind::BuildScript))
        .filter_map(|target| target.source.as_ref())
        .filter(|source| view.files.contains_key(source))
    {
        let reached = reach(view, source);
        found.overrides_environment |= reached.unresolved;
        for (at, file) in reached.files.iter().enumerate() {
            let text = String::from_utf8_lossy(&view.files[file]);
            let found_tokens = tokens(&text);
            found.tracks_changes |= at == 0 && declares(&found_tokens);
            found.overrides_environment |= overrides(&found_tokens);
        }
    }
    found
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
                .is_some_and(|bytes| !prints(&String::from_utf8_lossy(bytes), PREFIXES))
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

fn prints(source: &str, prefixes: &[&str]) -> bool {
    let found = tokens(source);
    (0..found.len()).any(|at| {
        matches!(found[at], Token::Ident(name) if PRINTING.contains(&name))
            && mentions(&found, at, prefixes)
    })
}

fn mentions(found: &[Token<'_>], at: usize, prefixes: &[&str]) -> bool {
    if found.get(at + 1) != Some(&Token::Punct('!')) {
        return false;
    }
    let mut depth = 0_usize;
    for token in found.iter().skip(at + 2) {
        match token {
            Token::Punct('(' | '[' | '{') => depth += 1,
            Token::Punct(')' | ']' | '}') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            Token::Str(text) if depth > 0 && prefixes.iter().any(|p| text.starts_with(p)) => {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn declares(found: &[Token<'_>]) -> bool {
    let mut depth = 0_usize;
    let mut mains = Vec::new();
    for (at, token) in found.iter().enumerate() {
        match token {
            Token::Punct('(' | '[' | '{') => depth += 1,
            Token::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
            Token::Ident("fn")
                if depth == 0
                    && matches!(
                        found[at + 1..],
                        [Token::Ident("main"), Token::Punct('('), ..]
                    ) =>
            {
                let open = found[at..].iter().position(|t| *t == Token::Punct('{'));
                mains.extend(open.map(|n| main_body(found, at + n)));
            }
            _ => {}
        }
    }
    !mains.is_empty() && mains.into_iter().all(top_level_print)
}

fn main_body<'t, 'a>(found: &'t [Token<'a>], open: usize) -> &'t [Token<'a>] {
    let end = group_end(found, open);
    &found[open + 1..end.saturating_sub(1).max(open + 1)]
}

fn top_level_print(body: &[Token<'_>]) -> bool {
    let mut depth = 0_usize;
    for (at, token) in body.iter().enumerate() {
        match token {
            Token::Punct('(' | '[' | '{') => depth += 1,
            Token::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
            Token::Ident("println" | "print")
                if depth == 0
                    && (at == 0 || matches!(body[at - 1], Token::Punct(';' | '}')))
                    && mentions(body, at, CHANGED) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn overrides(found: &[Token<'_>]) -> bool {
    let constants = constants(found);
    found
        .iter()
        .any(|token| matches!(token, Token::Str(text) if sets_reserved(text, &constants)))
}

fn constants<'t>(found: &[Token<'t>]) -> Vec<(&'t str, &'t str)> {
    let mut named = Vec::new();
    for (at, token) in found.iter().enumerate() {
        let [Token::Ident(name), rest @ ..] = &found[at + 1..] else {
            continue;
        };
        if *token != Token::Ident("const") {
            continue;
        }
        let end = rest.iter().position(|t| *t == Token::Punct(';'));
        if let Some([.., Token::Punct('='), Token::Str(value)]) = end.map(|n| &rest[..n]) {
            named.push((*name, *value));
        }
    }
    named
}

fn sets_reserved(literal: &str, constants: &[(&str, &str)]) -> bool {
    let mut rest = literal;
    while let Some(at) = rest.find(SET_ENV) {
        rest = &rest[at + SET_ENV.len()..];
        let Some((key, _)) = rest.split_once('=') else {
            return true;
        };
        if reserved(key, constants) {
            return true;
        }
    }
    false
}

fn reserved(key: &str, constants: &[(&str, &str)]) -> bool {
    let mut name = String::new();
    let mut rest = key;
    while let Some(open) = rest.find('{') {
        name.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else {
            return true;
        };
        let inner = &rest[open + 1..open + close];
        let Some((_, value)) = constants.iter().find(|(known, _)| *known == inner) else {
            return true;
        };
        name.push_str(value);
        rest = &rest[open + close + 1..];
    }
    name.push_str(rest);
    RESERVED.contains(&name.as_str())
}
