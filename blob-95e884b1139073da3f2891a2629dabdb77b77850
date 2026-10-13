//! Lock membership and reachability checks.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use std::fs;

use std::path::PathBuf;

use toml::Value;

use crate::context::FreshnessContext;

use super::{identity, lock_graph_subject, source, version};

use super::super::path_text;

pub(super) fn check_membership(
    ctx: &mut FreshnessContext,
    relative: &str,
    locked: &[Value],
    members: &BTreeSet<String>,
) {
    let local = locked
        .iter()
        .filter(|entry| entry.get("source").is_none())
        .filter_map(|entry| entry.get("name").and_then(Value::as_str).map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let subject = if relative.is_empty() {
        "(lock-membership)".to_owned()
    } else {
        format!("{relative}/(lock-membership)")
    };
    if local == *members {
        ctx.pass_row(
            "lock-staleness",
            &subject,
            &format!("{} members", members.len()),
        );
    } else {
        ctx.fail_row(
            "lock-staleness",
            &subject,
            &format!("local lock {local:?} != members {members:?}"),
        );
    }
}

pub(super) fn check_lock_graph(
    ctx: &mut FreshnessContext,
    relative: &str,
    locked: &[Value],
    by_name: &BTreeMap<String, Vec<Value>>,
    members: &BTreeSet<String>,
) {
    let local = locked
        .iter()
        .filter(|entry| entry.get("source").is_none())
        .filter_map(|entry| entry.get("name").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    let mut queue = local
        .intersection(&members.iter().map(String::as_str).collect())
        .flat_map(|name| by_name.get(*name).into_iter().flatten().cloned())
        .collect::<VecDeque<_>>();
    let mut reachable = queue.iter().map(identity).collect::<BTreeSet<_>>();
    while let Some(entry) = queue.pop_front() {
        let edges = entry
            .get("dependencies")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        for edge in edges.filter_map(Value::as_str) {
            let candidates = edge_candidates(edge, by_name);
            if candidates.is_empty() {
                ctx.fail_row(
                    "lock-staleness",
                    &lock_graph_subject(relative),
                    &format!(
                        "dangling edge {} -> {edge}",
                        entry
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("<missing>")
                    ),
                );
            }
            for candidate in candidates {
                if reachable.insert(identity(candidate)) {
                    queue.push_back(candidate.clone());
                }
            }
        }
    }
    let mut stranded = locked
        .iter()
        .filter(|entry| !reachable.contains(&identity(entry)))
        .collect::<Vec<_>>();
    stranded.sort_by_key(|entry| entry.get("name").and_then(Value::as_str).unwrap_or(""));
    if stranded.is_empty() {
        ctx.pass_row(
            "lock-staleness",
            &lock_graph_subject(relative),
            &format!("{} locked packages reachable", locked.len()),
        );
    } else {
        for entry in stranded {
            ctx.fail_row(
                "lock-staleness",
                &lock_graph_subject(relative),
                &format!(
                    "unreachable locked package {} {}",
                    entry
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("<missing>"),
                    version(entry).unwrap_or("<missing>")
                ),
            );
        }
    }
}

fn edge_candidates<'a>(edge: &str, by_name: &'a BTreeMap<String, Vec<Value>>) -> Vec<&'a Value> {
    let parts = edge.split(' ').collect::<Vec<_>>();
    by_name
        .get(parts[0])
        .into_iter()
        .flatten()
        .filter(|entry| {
            let correct_version = parts
                .get(1)
                .is_none_or(|want| version(entry) == Some(*want));
            let correct_source = parts.get(2).is_none_or(|want| {
                source(entry) == want.trim_matches(|ch| matches!(ch, '(' | ')'))
            });
            correct_version && correct_source
        })
        .collect()
}

pub(super) fn check_lock_mtime(ctx: &mut FreshnessContext, relative: &str, manifests: &[PathBuf]) {
    let lock = if relative.is_empty() {
        ctx.path("Cargo.lock")
    } else {
        ctx.path(relative).join("Cargo.lock")
    };
    let newest = manifests
        .iter()
        .filter_map(|path| {
            fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .ok()
        })
        .max();
    match (
        fs::metadata(&lock).and_then(|metadata| metadata.modified()),
        newest,
    ) {
        (Ok(lock_mtime), Some(manifest_mtime)) => ctx.info_row(
            "lock-mtime",
            &path_text(lock.strip_prefix(&ctx.root).unwrap_or(&lock)),
            &format!("lock_is_newest={}", lock_mtime >= manifest_mtime),
        ),
        (Err(error), _) => ctx.info_row(
            "lock-mtime",
            &path_text(lock.strip_prefix(&ctx.root).unwrap_or(&lock)),
            &format!("mtime unreadable ({error})"),
        ),
        (_, None) => ctx.info_row(
            "lock-mtime",
            &path_text(lock.strip_prefix(&ctx.root).unwrap_or(&lock)),
            "mtime unreadable (no member manifests)",
        ),
    }
}
