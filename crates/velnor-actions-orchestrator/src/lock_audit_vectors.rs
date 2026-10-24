//! `mise` argv classification for the install audit.

use super::MiseVectors;

/// Classify the `mise` vectors in one token run, in order.
///
/// Read the flags after each `mise` token.
/// For `install`, collect its tool specifications. For `exec`, continue to the next `mise` token.
/// Reject an unknown subcommand or an incomplete command.
/// Do not classify a bare `install` without a preceding `mise`, such as `cargo install`.
/// Stop collecting specifications at the first shell metacharacter or the end of the argument list.
pub(super) fn classify_mise_vectors(tokens: &[&str]) -> MiseVectors {
    for (at, token) in tokens.iter().enumerate() {
        if *token != "mise" {
            continue;
        }
        let mut cursor = at + 1;
        while cursor < tokens.len() && tokens[cursor].starts_with('-') {
            cursor += 1;
        }
        let Some(subcommand) = tokens.get(cursor) else {
            return MiseVectors::Unclassifiable("truncated".to_owned());
        };
        match *subcommand {
            "install" => return MiseVectors::Install(specs_until_metachar(&tokens[cursor + 1..])),
            "exec" => {}
            other => {
                return MiseVectors::Unclassifiable(format!("unexpected_subcommand:{other}"));
            }
        }
    }
    MiseVectors::NotInstall
}

/// Specs until the first shell metacharacter (scripts) or argv end.
fn specs_until_metachar(tail: &[&str]) -> Vec<String> {
    let mut specs = Vec::new();
    for spec in tail {
        if matches!(*spec, "&&" | ";" | "||" | "}" | "{" | "(" | ")") {
            break;
        }
        specs.push((*spec).to_owned());
    }
    specs
}
