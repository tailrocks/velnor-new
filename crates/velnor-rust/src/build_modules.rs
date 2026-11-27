// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{RepoPath, RepoView};

use crate::doc_steps::tokens::{Token, tokens};
use crate::embedded::resolve;

#[derive(Debug, Default)]
pub(crate) struct Reach {
    pub(crate) files: Vec<RepoPath>,
    pub(crate) unresolved: bool,
}

struct Pending {
    path: RepoPath,
    children: Vec<String>,
}

enum Attribute<'t> {
    Absent,
    Path(&'t str),
    Opaque,
}

enum Link<'t> {
    Module(&'t str, Attribute<'t>),
    Include(&'t str),
    Opaque,
}

pub(crate) fn reach(view: &RepoView, root: &RepoPath) -> Reach {
    let mut reach = Reach::default();
    let mut seen = BTreeSet::new();
    let mut pending = vec![Pending {
        path: root.clone(),
        children: directory(root),
    }];
    while let Some(Pending { path, children }) = pending.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let Some(bytes) = view.files.get(&path) else {
            reach.unresolved = true;
            continue;
        };
        reach.files.push(path.clone());
        let text = String::from_utf8_lossy(bytes);
        let own = directory(&path);
        for link in links(&tokens(&text)) {
            let next = match link {
                Link::Module(_, Attribute::Path(literal)) => {
                    resolve(view, &borrowed(&own), literal).map(|found| (directory(&found), found))
                }
                Link::Module(name, Attribute::Absent) => default_module(view, &children, name),
                Link::Include(literal) => {
                    resolve(view, &borrowed(&own), literal).map(|found| (children.clone(), found))
                }
                Link::Module(_, Attribute::Opaque) | Link::Opaque => None,
            };
            match next {
                Some((children, path)) => pending.push(Pending { path, children }),
                None => reach.unresolved = true,
            }
        }
    }
    reach
}

fn default_module(
    view: &RepoView,
    children: &[String],
    name: &str,
) -> Option<(Vec<String>, RepoPath)> {
    let base = borrowed(children);
    let found = resolve(view, &base, &format!("{name}.rs"))
        .or_else(|| resolve(view, &base, &format!("{name}/mod.rs")))?;
    let mut inner = children.to_vec();
    inner.push(name.to_owned());
    Some((inner, found))
}

fn borrowed(parts: &[String]) -> Vec<&str> {
    parts.iter().map(String::as_str).collect()
}

fn directory(file: &RepoPath) -> Vec<String> {
    let mut parts: Vec<String> = file.as_str().split('/').map(str::to_owned).collect();
    parts.pop();
    parts
}

fn links<'t>(found: &[Token<'t>]) -> Vec<Link<'t>> {
    let mut depth = 0_usize;
    let mut links = Vec::new();
    for (at, token) in found.iter().enumerate() {
        match (token, &found[at + 1..]) {
            (Token::Punct('{'), _) => depth += 1,
            (Token::Punct('}'), _) => depth = depth.saturating_sub(1),
            (Token::Ident("mod"), [Token::Ident(name), Token::Punct(';'), ..]) => {
                links.push(if depth == 0 {
                    Link::Module(name, attribute(&found[..at]))
                } else {
                    Link::Opaque
                });
            }
            (
                Token::Ident("include"),
                [
                    Token::Punct('!'),
                    Token::Punct('(' | '[' | '{'),
                    Token::Str(literal),
                    Token::Punct(')' | ']' | '}'),
                    ..,
                ],
            ) => links.push(Link::Include(literal)),
            (Token::Ident("include"), [Token::Punct('!'), ..]) => links.push(Link::Opaque),
            _ => {}
        }
    }
    links
}

fn attribute<'t>(before: &[Token<'t>]) -> Attribute<'t> {
    let mut end = before.len();
    match before.last() {
        Some(Token::Ident("pub")) => end -= 1,
        Some(Token::Punct(')')) => {
            let open = before.iter().rposition(|token| *token == Token::Punct('('));
            if let Some(open) = open.filter(|open| *open > 0)
                && before[open - 1] == Token::Ident("pub")
            {
                end = open - 1;
            }
        }
        _ => {}
    }
    let mut found = Attribute::Absent;
    while before[..end].last() == Some(&Token::Punct(']')) {
        let Some(open) = before[..end]
            .iter()
            .rposition(|token| *token == Token::Punct('['))
        else {
            return Attribute::Opaque;
        };
        match &before[open + 1..end - 1] {
            [Token::Ident("path"), Token::Punct('='), Token::Str(literal)] => {
                found = Attribute::Path(literal);
            }
            inner if inner.contains(&Token::Ident("path")) => return Attribute::Opaque,
            _ => {}
        }
        end = open.saturating_sub(1);
    }
    found
}
