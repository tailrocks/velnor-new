// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::error::CiError;
use crate::github::{ApiRule, GitHub, Repo, parse, run_path};

const PAGE: usize = 100;
const MAX_PAGES: u32 = 5;
const REQUIRED_RULES: [&str; 3] = ["creation", "update", "deletion"];

#[derive(Debug)]
pub struct Release<'a> {
    pub tag: &'a str,
    pub actor: &'a str,
    pub actors: &'a [String],
    pub run_id: Option<u64>,
}

#[derive(Deserialize)]
struct Listed {
    id: u64,
    target: String,
    enforcement: String,
}

#[derive(Deserialize)]
struct Ruleset {
    id: u64,
    name: String,
    target: String,
    enforcement: String,
    created_at: Option<String>,
    updated_at: Option<String>,
    conditions: Option<Conditions>,
    #[serde(default)]
    rules: Vec<ApiRule>,
}

#[derive(Deserialize)]
struct Conditions {
    ref_name: Option<RefName>,
}

#[derive(Deserialize)]
struct RefName {
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}

#[derive(Deserialize)]
struct Run {
    created_at: String,
}

fn supported(pattern: &str) -> bool {
    pattern == "~ALL"
        || (!pattern.is_empty()
            && pattern
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_./*?".contains(&byte)))
}

fn matches(pattern: &str, reference: &str) -> bool {
    pattern == "~ALL" || glob(pattern.as_bytes(), reference.as_bytes())
}

fn stamp(text: &str) -> Option<&str> {
    let shape = b"dddd-dd-ddTdd:dd:ddZ";
    (text.len() == shape.len()
        && text.bytes().zip(shape).all(|(byte, kind)| match kind {
            b'd' => byte.is_ascii_digit(),
            other => byte == *other,
        }))
    .then_some(text)
}

fn predates(ruleset: &Ruleset, pushed: Option<&str>) -> bool {
    let changed = ruleset
        .updated_at
        .as_deref()
        .or(ruleset.created_at.as_deref())
        .and_then(stamp);
    matches!((changed, pushed.and_then(stamp)), (Some(changed), Some(pushed)) if changed <= pushed)
}

fn pushed_at(github: &dyn GitHub, repo: &Repo, run_id: u64) -> Result<String, CiError> {
    let run: Run = parse(&github.get(&run_path(repo, run_id))?, "workflow run")?;
    Ok(run.created_at)
}

fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => {
            let (deep, rest) = match rest.split_first() {
                Some((b'*', rest)) => (true, rest),
                _ => (false, rest),
            };
            (0..=text.len())
                .any(|taken| (deep || !text[..taken].contains(&b'/')) && glob(rest, &text[taken..]))
        }
        Some((b'?', rest)) => text
            .split_first()
            .is_some_and(|(head, tail)| *head != b'/' && glob(rest, tail)),
        Some((byte, rest)) => text
            .split_first()
            .is_some_and(|(head, tail)| head == byte && glob(rest, tail)),
    }
}

fn protects(ruleset: &Ruleset, reference: &str) -> bool {
    let Some(names) = ruleset
        .conditions
        .as_ref()
        .and_then(|conditions| conditions.ref_name.as_ref())
    else {
        return false;
    };
    ruleset.target == "tag"
        && ruleset.enforcement == "active"
        && names
            .include
            .iter()
            .chain(&names.exclude)
            .all(|glob| supported(glob))
        && names.include.iter().any(|glob| matches(glob, reference))
        && !names.exclude.iter().any(|glob| matches(glob, reference))
        && REQUIRED_RULES
            .iter()
            .all(|required| ruleset.rules.iter().any(|rule| rule.kind == *required))
}

fn listed(github: &dyn GitHub, repo: &Repo) -> Result<Vec<Listed>, CiError> {
    let mut all = Vec::new();
    for page in 1..=MAX_PAGES {
        let items: Vec<Listed> = parse(
            &github.get(&format!(
                "/repos/{repo}/rulesets?includes_parents=true&per_page={PAGE}&page={page}"
            ))?,
            "rulesets",
        )?;
        let last = items.len() < PAGE;
        all.extend(items);
        if last {
            return Ok(all);
        }
    }
    Err(CiError::new(format!(
        "{repo} lists more than {} rulesets; refusing to guess whether the release tag is protected",
        PAGE * MAX_PAGES as usize
    )))
}

fn protecting(
    github: &dyn GitHub,
    repo: &Repo,
    reference: &str,
    pushed: Option<&str>,
) -> Result<Option<String>, CiError> {
    for item in listed(github, repo)? {
        if item.target != "tag" || item.enforcement != "active" {
            continue;
        }
        let detail: Ruleset = parse(
            &github.get(&format!("/repos/{repo}/rulesets/{}", item.id))?,
            "tag ruleset",
        )?;
        if detail.id == item.id && protects(&detail, reference) && predates(&detail, pushed) {
            return Ok(Some(detail.name));
        }
    }
    Ok(None)
}

pub fn authorize(
    github: &dyn GitHub,
    repo: &Repo,
    release: &Release<'_>,
) -> Result<String, CiError> {
    let Release {
        tag,
        actor,
        actors,
        run_id,
    } = release;
    if tag.is_empty() || actor.is_empty() {
        return Err(CiError::new("the tag and the actor must not be empty"));
    }
    if actors
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(actor))
    {
        return Ok(format!(
            "tag {tag} was pushed by {actor}, an allowed release actor\n"
        ));
    }
    let pushed = run_id.map(|id| pushed_at(github, repo, id)).transpose()?;
    match protecting(github, repo, &format!("refs/tags/{tag}"), pushed.as_deref())? {
        Some(name) => Ok(format!(
            "tag {tag} is covered by the active tag ruleset {name}, which forbids creating, moving and deleting it\n"
        )),
        None => Err(CiError::new(format!(
            "{actor} is not an allowed release actor and no active tag ruleset of {repo}, last changed before the tag was pushed, forbids creating, moving and deleting {tag}; list the actor in release_actors or protect the tag with a ruleset (creation, update and deletion rules) before the tag is pushed"
        ))),
    }
}
