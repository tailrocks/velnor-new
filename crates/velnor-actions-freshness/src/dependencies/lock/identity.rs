//! Declaration-to-lock resolution checks.

use std::collections::{BTreeMap, BTreeSet};

use toml::Value;

use crate::context::FreshnessContext;

use super::super::{CRATES_IO, DeclaredDependency};

use super::{source, version};

pub(super) fn check_declared(
    ctx: &mut FreshnessContext,
    relative: &str,
    declared: &[DeclaredDependency],
    by_name: &BTreeMap<String, Vec<Value>>,
    members: &BTreeSet<String>,
) {
    let mut passed = 0;
    let mut failed = 0;
    for dependency in declared {
        if check_requirement(ctx, dependency, by_name, members) {
            passed += 1;
        } else {
            failed += 1;
        }
    }
    let subject = if relative.is_empty() {
        "(declared-summary)".to_owned()
    } else {
        format!("{relative}/(declared-summary)")
    };
    ctx.info_row(
        "lock-staleness",
        &subject,
        &format!(
            "{passed} match, {failed} fail, {} locked names retained",
            by_name.len()
        ),
    );
}

fn check_requirement(
    ctx: &mut FreshnessContext,
    dependency: &DeclaredDependency,
    by_name: &BTreeMap<String, Vec<Value>>,
    members: &BTreeSet<String>,
) -> bool {
    let Some(want) = dependency.requirement.strip_prefix('=') else {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!(
                "requirement {:?} is not exact `=x.y.z` (VER-2.26)",
                dependency.requirement
            ),
        );
        return false;
    };
    let want = want.split('+').next().unwrap_or("");
    let matches = by_name
        .get(&dependency.name)
        .into_iter()
        .flatten()
        .filter(|entry| version(entry) == Some(want))
        .cloned()
        .collect::<Vec<_>>();
    if matches.is_empty() {
        let locked = by_name
            .get(&dependency.name)
            .into_iter()
            .flatten()
            .filter_map(|entry| version(entry))
            .collect::<BTreeSet<_>>();
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!(
                "declared {:?} has no locked identity (locked versions: {locked:?})",
                dependency.requirement
            ),
        );
        return false;
    }
    check_resolved_source(ctx, dependency, &matches, members)
}

fn check_resolved_source(
    ctx: &mut FreshnessContext,
    dependency: &DeclaredDependency,
    matches: &[Value],
    members: &BTreeSet<String>,
) -> bool {
    let sources = matches.iter().map(source).collect::<BTreeSet<_>>();
    if sources.len() != 1 {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!(
                "ambiguous identity: {} {} resolves from {sources:?}",
                dependency.name,
                dependency.requirement.trim_start_matches('=')
            ),
        );
        return false;
    }
    let resolved_source = sources.iter().next().map_or("local", String::as_str);
    if members.contains(&dependency.name) && resolved_source != "local" {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!(
                "workspace member {} locked from {resolved_source}",
                dependency.name
            ),
        );
        return false;
    }
    if members.contains(&dependency.name) {
        ctx.pass_row(
            "lock-staleness",
            &dependency.subject,
            &format!("{} @ workspace", dependency.requirement),
        );
        true
    } else if resolved_source != CRATES_IO {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!("locked from non-registry source {resolved_source}"),
        );
        false
    } else {
        ctx.pass_row(
            "lock-staleness",
            &dependency.subject,
            &format!("{} @ registry", dependency.requirement),
        );
        true
    }
}
