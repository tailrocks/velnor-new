// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{CompileInputs, InputOwner, RepoPath, RepoView, RustMember, RustRoot};

use crate::doc_steps::tokens::{Token, tokens};
use crate::error::DetectError;
use crate::reads::{INPUTS_LOCK, parse_inputs_lock};
use crate::select::relative;

enum Argument<'a> {
    Relative(&'a str),
    ManifestDir(&'a str),
    BuildOutput,
    Unknown,
}

impl<'a> Argument<'a> {
    fn parse(first: &Token<'a>, rest: &[Token<'a>]) -> Self {
        match (first, rest) {
            (Token::Str(literal), _) => Self::Relative(literal),
            (
                Token::Ident("concat"),
                [
                    Token::Punct('!'),
                    Token::Punct('('),
                    Token::Ident("env"),
                    Token::Punct('!'),
                    Token::Punct('('),
                    Token::Str(variable),
                    Token::Punct(')'),
                    Token::Punct(','),
                    Token::Str(tail),
                    ending @ ..,
                ],
            ) if tail.starts_with('/') && !tail.contains('\\') && closes(ending) => match *variable
            {
                "CARGO_MANIFEST_DIR" => Self::ManifestDir(tail),
                "OUT_DIR" if !tail.split('/').any(|part| part == "..") => Self::BuildOutput,
                _ => Self::Unknown,
            },
            _ => Self::Unknown,
        }
    }
}

fn closes(ending: &[Token<'_>]) -> bool {
    matches!(
        ending,
        [Token::Punct(')'), ..] | [Token::Punct(','), Token::Punct(')'), ..]
    )
}

const MACROS: &[&str] = &["include_str", "include_bytes", "include"];

#[must_use]
pub(crate) fn embedded_inputs(view: &RepoView, roots: &[RustRoot]) -> CompileInputs {
    let members: Vec<&RustMember> = roots.iter().flat_map(|root| &root.members).collect();
    let mut inputs = CompileInputs::new();
    for member in &members {
        for key in Scan::new(view, member, &members).run() {
            inputs
                .entry(key)
                .or_default()
                .insert(InputOwner::at(member.name.clone(), member.manifest.clone()));
        }
    }
    inputs
}

struct Scan<'a> {
    view: &'a RepoView,
    directory: Option<RepoPath>,
    reads: BTreeSet<Option<RepoPath>>,
    seen: BTreeSet<&'a RepoPath>,
    pending: Vec<&'a RepoPath>,
}

impl<'a> Scan<'a> {
    fn new(view: &'a RepoView, member: &RustMember, members: &[&RustMember]) -> Self {
        let nested: Vec<&RepoPath> = members
            .iter()
            .filter(|other| other.manifest != member.manifest)
            .filter_map(|other| other.directory.as_ref())
            .collect();
        let mut bases = vec![member.directory.clone()];
        bases.extend(
            member
                .targets
                .iter()
                .filter_map(|target| target.source.as_ref())
                .filter(|source| relative(member.directory.as_ref(), source).is_none())
                .map(RepoPath::parent),
        );
        let pending = view
            .tracked_paths
            .iter()
            .filter(|path| is_rust_source(path))
            .filter(|path| {
                bases.iter().any(|base| {
                    relative(base.as_ref(), path).is_some()
                        && !nested.iter().any(|directory| {
                            Some(*directory) != base.as_ref()
                                && relative(base.as_ref(), directory).is_some()
                                && relative(Some(directory), path).is_some()
                        })
                })
            })
            .collect();
        Self {
            view,
            directory: member.directory.clone(),
            reads: BTreeSet::new(),
            seen: BTreeSet::new(),
            pending,
        }
    }

    fn run(mut self) -> BTreeSet<Option<RepoPath>> {
        while let Some(file) = self.pending.pop() {
            if !self.seen.insert(file) {
                continue;
            }
            match self.view.files.get(file) {
                Some(bytes) => self.scan(file, &String::from_utf8_lossy(bytes)),
                None if self.view.symlinks.contains(file) => {
                    self.reads.insert(None);
                }
                None => {}
            }
        }
        self.reads
    }

    fn scan(&mut self, file: &RepoPath, text: &str) {
        let tokens = tokens(text);
        for (at, token) in tokens.iter().enumerate() {
            let Token::Ident(name) = token else { continue };
            if !MACROS.contains(name) {
                continue;
            }
            let [
                Token::Punct('!'),
                Token::Punct('(' | '[' | '{'),
                argument,
                ..,
            ] = &tokens[at + 1..]
            else {
                continue;
            };
            let resolved = match Argument::parse(argument, &tokens[at + 4..]) {
                Argument::Relative(literal) => {
                    literal_path(literal).and_then(|path| resolve(file, path))
                }
                Argument::ManifestDir(rest) => self.under_manifest_dir(rest),
                Argument::BuildOutput => continue,
                Argument::Unknown => None,
            };
            let Some(resolved) = resolved else {
                self.reads.insert(None);
                continue;
            };
            if self.through_symlink(&resolved) {
                self.reads.insert(None);
                continue;
            }
            if *name == "include" {
                self.follow(&resolved);
            }
            self.reads.insert(Some(resolved));
        }
    }

    fn through_symlink(&self, path: &RepoPath) -> bool {
        let mut at = Some(path.clone());
        while let Some(current) = at {
            if self.view.symlinks.contains(&current) {
                return true;
            }
            at = current.parent();
        }
        false
    }

    fn under_manifest_dir(&self, rest: &str) -> Option<RepoPath> {
        let parts = self.directory.as_ref().map_or_else(Vec::new, |directory| {
            directory.as_str().split('/').collect()
        });
        join(parts, rest.trim_start_matches('/'))
    }

    fn follow(&mut self, included: &RepoPath) {
        let held = self
            .view
            .files
            .get_key_value(included)
            .map(|(path, _)| path);
        let plain = self
            .view
            .tracked_paths
            .get(included)
            .filter(|path| is_rust_source(path));
        match held.or(plain) {
            Some(path) => self.pending.push(path),
            None => {
                self.reads.insert(None);
            }
        }
    }
}

fn resolve(file: &RepoPath, literal: &str) -> Option<RepoPath> {
    if literal.starts_with('/') || literal.contains('\\') {
        return None;
    }
    let mut parts: Vec<&str> = file.as_str().split('/').collect();
    parts.pop();
    join(parts, literal)
}

fn join<'a>(mut parts: Vec<&'a str>, literal: &'a str) -> Option<RepoPath> {
    for part in literal.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    RepoPath::new(parts.join("/")).ok()
}

fn literal_path(literal: &str) -> Option<&str> {
    (!literal.contains('\\')).then_some(literal)
}

#[must_use]
pub fn is_rust_source(path: &RepoPath) -> bool {
    std::path::Path::new(path.as_str())
        .extension()
        .is_some_and(|extension| extension == "rs")
}

pub(crate) fn locked_and_embedded(
    view: &RepoView,
    roots: &[RustRoot],
) -> Result<CompileInputs, DetectError> {
    let mut inputs = view.files.get(&INPUTS_LOCK).map_or_else(
        || Ok(CompileInputs::new()),
        |bytes| parse_inputs_lock(bytes),
    )?;
    for (file, owners) in embedded_inputs(view, roots) {
        inputs.entry(file).or_default().extend(owners);
    }
    Ok(inputs)
}

#[must_use]
pub fn is_plain_source(path: &RepoPath, bytes: &[u8]) -> bool {
    is_rust_source(path) && !bytes.windows(7).any(|window| window == b"include")
}
