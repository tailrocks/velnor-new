// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{FetchedManifest, RepoPath};

use crate::expression_length::{MAX_EXPRESSION_CHARACTERS, joined_length};
use crate::gate::{ON_DEFAULT_BRANCH, gate_parts};
use crate::helper_steps::hex_path;
use crate::private_steps::TOKEN_STEP;
use crate::release_steps::shell_word;
use crate::rules::{MAX_RUN_CHARACTERS, RenderedStep, Value};

const FETCH_MANIFESTS: &str = "VELNOR_FETCH_MANIFESTS";

struct Entry<'a> {
    path: &'a str,
    when: Option<String>,
}

pub(super) fn selected(manifests: &[FetchedManifest], listed: bool) -> RenderedStep {
    let entries = manifests
        .iter()
        .map(|manifest| Entry {
            path: manifest.path.as_str(),
            when: manifest.selected_by.as_ref().map(|gate| {
                format!(
                    "${{{{ {ON_DEFAULT_BRANCH} || ({}) }}}}",
                    gate_parts(gate, false).join(" && ")
                )
            }),
        })
        .collect::<Vec<_>>();
    step(&entries, listed)
}

pub(super) fn one(manifest: &str) -> RenderedStep {
    step(
        &[Entry {
            path: manifest,
            when: None,
        }],
        false,
    )
}

pub(super) fn every(manifests: &[RepoPath], listed: bool) -> RenderedStep {
    let entries = manifests
        .iter()
        .map(|manifest| Entry {
            path: manifest.as_str(),
            when: None,
        })
        .collect::<Vec<_>>();
    step(&entries, listed)
}

fn step(entries: &[Entry<'_>], listed: bool) -> RenderedStep {
    let mut plain = true;
    let arguments = entries
        .iter()
        .map(|entry| {
            let word = shell_word(entry.path);
            plain &= word == entry.path;
            format!("--manifest-path {word}")
        })
        .collect::<Vec<_>>()
        .join(" ");
    let run = format!("velnor-ci fetch {arguments}");
    let too_long = crate::helper_integrity::INVOCATION_PREFIX.len() + run.len()
        - "velnor-ci ".len()
        > MAX_RUN_CHARACTERS;
    let conditional = entries.iter().any(|entry| entry.when.is_some());
    let (run, variable) = if listed && (too_long || conditional) {
        (
            format!("velnor-ci fetch --manifests-env {FETCH_MANIFESTS}"),
            Some((FETCH_MANIFESTS.to_owned(), pairs(entries))),
        )
    } else if too_long && plain {
        (
            format!("velnor-ci fetch ${FETCH_MANIFESTS}"),
            Some((FETCH_MANIFESTS.to_owned(), Value::Text(arguments))),
        )
    } else {
        (run, None)
    };
    RenderedStep {
        name: "Fetch the dependencies, then drop the token".to_owned(),
        env: variable
            .into_iter()
            .chain([
                (
                    "VELNOR_PRIVATE_TOKEN".to_owned(),
                    Value::Expr(format!("${{{{ steps.{TOKEN_STEP}.outputs.token }}}}")),
                ),
                (
                    "VELNOR_API_URL".to_owned(),
                    Value::Expr("${{ github.api_url }}".to_owned()),
                ),
                (
                    "VELNOR_SERVER_URL".to_owned(),
                    Value::Expr("${{ github.server_url }}".to_owned()),
                ),
            ])
            .collect(),
        run: Some(run),
        ..RenderedStep::default()
    }
}

fn pairs(entries: &[Entry<'_>]) -> Value {
    let list = |answer: &dyn Fn(&Entry<'_>) -> String| {
        let items: Vec<String> = entries
            .iter()
            .map(|entry| format!("[\"{}\",{}]", hex_path(entry.path), answer(entry)))
            .collect();
        format!("[{}]", items.join(","))
    };
    let text = list(&|entry| entry.when.clone().unwrap_or_else(|| "true".to_owned()));
    if entries.iter().all(|entry| entry.when.is_none())
        || joined_length(&text) > MAX_EXPRESSION_CHARACTERS
    {
        return Value::Text(list(&|_| "true".to_owned()));
    }
    Value::BoundedExpr {
        max_bytes: list(&|_| "false".to_owned()).len(),
        text,
    }
}
