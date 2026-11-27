// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{
    CompileInputs, HelperRelease, InputKey, InputOwner, RepoPath, RepoView, RustMember, RustRoot,
};

use crate::audit_job::pull_request_audit;
use crate::doc_steps::tokens::{Token, tokens};
use crate::error::DetectError;
use crate::lock::{LOCK_PATH, parse_lock};
use crate::reads::{INPUTS_LOCK, parse_inputs_lock};
use crate::select::relative;

const MACROS: &[&str] = &["include_str", "include_bytes", "include"];

#[must_use]
pub(crate) fn embedded_inputs(
    view: &RepoView,
    roots: &[RustRoot],
    evidence: IncludeEvidence,
) -> CompileInputs {
    let members: Vec<&RustMember> = roots.iter().flat_map(|root| &root.members).collect();
    let mut inputs = CompileInputs::new();
    for member in &members {
        for key in Scan::new(view, member, &members, evidence).run() {
            inputs
                .entry(key)
                .or_default()
                .insert(InputOwner::at(member.name.clone(), member.manifest.clone()));
        }
    }
    inputs
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum IncludeEvidence {
    Audited,
    SourceOnly,
}

impl IncludeEvidence {
    pub(crate) fn of(helper: Option<&HelperRelease>) -> Self {
        if pull_request_audit(helper) {
            Self::Audited
        } else {
            Self::SourceOnly
        }
    }
}

enum Embedded<'t> {
    Literal(&'t str),
    ManifestDir(String),
    Output,
    Unresolved,
}

struct Scan<'a> {
    view: &'a RepoView,
    directory: Option<&'a RepoPath>,
    evidence: IncludeEvidence,
    reads: BTreeSet<InputKey>,
    seen: BTreeSet<&'a RepoPath>,
    pending: Vec<&'a RepoPath>,
}

impl<'a> Scan<'a> {
    fn new(
        view: &'a RepoView,
        member: &'a RustMember,
        members: &[&RustMember],
        evidence: IncludeEvidence,
    ) -> Self {
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
            directory: member.directory.as_ref(),
            evidence,
            reads: BTreeSet::new(),
            seen: BTreeSet::new(),
            pending,
        }
    }

    fn run(mut self) -> BTreeSet<InputKey> {
        while let Some(file) = self.pending.pop() {
            if !self.seen.insert(file) {
                continue;
            }
            match self.view.files.get(file) {
                Some(bytes) => self.scan(file, &String::from_utf8_lossy(bytes)),
                None if self.view.symlinks.contains(file) => {
                    self.reads.insert(InputKey::Unresolved);
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
                argument @ ..,
            ] = &tokens[at + 1..]
            else {
                continue;
            };
            let audited = self.evidence == IncludeEvidence::Audited;
            let resolved = match embedded(argument) {
                Embedded::Literal(literal) => literal_path(literal)
                    .and_then(|path| resolve(self.view, &directory_of(file), path)),
                Embedded::ManifestDir(joined) if audited => joined
                    .strip_prefix('/')
                    .and_then(|path| resolve(self.view, &self.directory_parts(), path)),
                Embedded::Output if audited => continue,
                Embedded::ManifestDir(_) | Embedded::Output | Embedded::Unresolved => None,
            };
            let Some(resolved) = resolved else {
                self.reads.insert(InputKey::Unresolved);
                continue;
            };
            if *name == "include" {
                self.follow(&resolved);
            }
            self.reads.insert(InputKey::Path(resolved));
        }
    }

    fn directory_parts(&self) -> Vec<&'a str> {
        self.directory.map_or_else(Vec::new, |directory| {
            directory.as_str().split('/').collect()
        })
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
                self.reads.insert(InputKey::Unresolved);
            }
        }
    }
}

fn embedded<'t>(argument: &[Token<'t>]) -> Embedded<'t> {
    match argument {
        [Token::Str(literal), ..] => Embedded::Literal(literal),
        [
            Token::Ident("concat"),
            Token::Punct('!'),
            Token::Punct('('),
            Token::Ident("env"),
            Token::Punct('!'),
            Token::Punct('('),
            Token::Str(variable),
            Token::Punct(')'),
            rest @ ..,
        ] => match (*variable, literals(rest)) {
            ("CARGO_MANIFEST_DIR", Some(joined)) => Embedded::ManifestDir(joined),
            ("OUT_DIR", Some(joined)) if !joined.split('/').any(|part| part == "..") => {
                Embedded::Output
            }
            _ => Embedded::Unresolved,
        },
        _ => Embedded::Unresolved,
    }
}

fn literals(rest: &[Token<'_>]) -> Option<String> {
    let mut joined = String::new();
    let mut rest = rest;
    loop {
        match rest {
            [Token::Punct(','), Token::Str(literal), tail @ ..] => {
                joined.push_str(literal_path(literal)?);
                rest = tail;
            }
            [Token::Punct(')'), ..] | [Token::Punct(','), Token::Punct(')'), ..] => {
                return Some(joined);
            }
            _ => return None,
        }
    }
}

fn directory_of(file: &RepoPath) -> Vec<&str> {
    let mut parts: Vec<&str> = file.as_str().split('/').collect();
    parts.pop();
    parts
}

pub(crate) fn resolve(view: &RepoView, base: &[&str], literal: &str) -> Option<RepoPath> {
    if literal.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in base.iter().copied().chain(literal.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
        if !parts.is_empty() && is_linked(view, &parts) {
            return None;
        }
    }
    RepoPath::new(parts.join("/"))
        .ok()
        .filter(|path| view.tracked_paths.contains(path))
}

fn is_linked(view: &RepoView, parts: &[&str]) -> bool {
    RepoPath::new(parts.join("/"))
        .is_ok_and(|prefix| view.symlinks.contains(&prefix) || view.gitlinks.contains(&prefix))
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
    let evidence = IncludeEvidence::of(
        view.files
            .get(&LOCK_PATH)
            .and_then(|bytes| parse_lock(bytes).ok())
            .as_ref(),
    );
    let mut inputs = view.files.get(&INPUTS_LOCK).map_or_else(
        || Ok(CompileInputs::new()),
        |bytes| parse_inputs_lock(bytes),
    )?;
    for (file, owners) in embedded_inputs(view, roots, evidence) {
        inputs.entry(file).or_default().extend(owners);
    }
    Ok(inputs)
}

#[must_use]
pub fn is_plain_source(path: &RepoPath, bytes: &[u8]) -> bool {
    is_rust_source(path) && !bytes.windows(7).any(|window| window == b"include")
}
