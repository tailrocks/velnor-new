//! Project dependency edges read from a build script.
//!
//! Two literal spellings are recognised: `project(":x")` and the type-safe
//! accessors `projects.fooBar` (kebab, snake and dot segments map to camel
//! case: `:eth-domain` is `projects.ethDomain`, `:a:b` is `projects.a.b`).

use std::collections::BTreeSet;

use crate::lex::{Tok, tokenize};

/// Type-safe accessor spelling of a project path (without `projects.`).
#[must_use]
pub fn accessor(path: &str) -> String {
    path.split(':')
        .filter(|segment| !segment.is_empty())
        .map(camel)
        .collect::<Vec<_>>()
        .join(".")
}

fn camel(segment: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for ch in segment.chars() {
        if matches!(ch, '-' | '_' | '.') {
            upper = !out.is_empty();
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Known project paths a build script depends on, sorted and de-duplicated.
#[must_use]
pub fn project_edges(script: &str, known: &BTreeSet<String>) -> BTreeSet<String> {
    let toks = tokenize(script);
    let by_accessor: Vec<(String, &String)> =
        known.iter().map(|path| (accessor(path), path)).collect();
    let mut out = BTreeSet::new();
    for (i, tok) in toks.iter().enumerate() {
        match tok {
            Tok::Ident(name) if name == "project" => {
                if let (Some(Tok::Punct('(')), Some(Tok::Str(path, false))) =
                    (toks.get(i + 1), toks.get(i + 2))
                {
                    let path = if path.starts_with(':') {
                        path.clone()
                    } else {
                        format!(":{path}")
                    };
                    if known.contains(&path) {
                        out.insert(path);
                    }
                }
            }
            Tok::Ident(name)
                if name == "projects"
                    && !matches!(
                        i.checked_sub(1).and_then(|p| toks.get(p)),
                        Some(Tok::Punct('.'))
                    ) =>
            {
                out.extend(accessor_target(&toks[i + 1..], &by_accessor));
            }
            _ => {}
        }
    }
    out
}

/// Longest accessor chain after `projects`.
fn accessor_target(rest: &[Tok], known: &[(String, &String)]) -> Option<String> {
    let mut chain: Vec<&str> = Vec::new();
    let mut k = 0;
    while let (Some(Tok::Punct('.')), Some(Tok::Ident(name))) = (rest.get(k), rest.get(k + 1)) {
        chain.push(name);
        k += 2;
    }
    (1..=chain.len()).rev().find_map(|len| {
        let spelled = chain[..len].join(".");
        known
            .iter()
            .find(|(accessor, _)| *accessor == spelled)
            .map(|(_, path)| (*path).clone())
    })
}
