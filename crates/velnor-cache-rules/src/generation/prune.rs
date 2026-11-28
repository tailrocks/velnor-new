// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::entry::Timestamp;
use crate::generation::key::GenKey;
use crate::generation::scope::{GenScope, Generation};

const HOUR: u64 = 60 * 60;
const DAY: u64 = 24 * HOUR;

pub const PRUNE_FLOOR_SECONDS: u64 = 6 * HOUR;
pub const MAIN_KEEP: usize = 8;
pub const PR_KEEP: usize = 2;
pub const MAIN_MAX_BORN_SECONDS: u64 = 7 * DAY;
pub const PR_MAX_SECONDS: u64 = 7 * DAY;
pub const MG_MAX_SECONDS: u64 = DAY;

fn age(now: Timestamp, then: Timestamp) -> u64 {
    now.0.saturating_sub(then.0)
}

fn expired(now: Timestamp, generation: &Generation) -> bool {
    match generation.scope {
        GenScope::Main { .. } => age(now, generation.born) > MAIN_MAX_BORN_SECONDS,
        GenScope::Pr { .. } => age(now, generation.published) > PR_MAX_SECONDS,
        GenScope::Mg { .. } => age(now, generation.published) > MG_MAX_SECONDS,
    }
}

fn newest_first(a: &Generation, b: &Generation) -> Ordering {
    match (&a.scope, &b.scope) {
        (GenScope::Pr { .. }, GenScope::Pr { .. }) => b.scope.cmp(&a.scope),
        _ => (b.published, &b.scope).cmp(&(a.published, &a.scope)),
    }
}

fn newest_main_per_key(generations: &[Generation]) -> BTreeSet<usize> {
    let mut best: BTreeMap<&GenKey, usize> = BTreeMap::new();
    for (index, generation) in generations.iter().enumerate() {
        if matches!(generation.scope, GenScope::Main { .. }) {
            let slot = best.entry(&generation.key).or_insert(index);
            if newest_first(generation, &generations[*slot]) == Ordering::Less {
                *slot = index;
            }
        }
    }
    best.into_values().collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrunePlan<'a> {
    pub victims: Vec<&'a Generation>,
    pub eviction_queue: Vec<&'a Generation>,
}

fn newest_per_key_without_main(
    generations: &[Generation],
    survivors: impl Fn(usize) -> bool,
) -> BTreeSet<usize> {
    let mut best: BTreeMap<&GenKey, usize> = BTreeMap::new();
    let mut has_main: BTreeSet<&GenKey> = BTreeSet::new();
    for (index, generation) in generations.iter().enumerate() {
        if matches!(generation.scope, GenScope::Main { .. }) {
            has_main.insert(&generation.key);
        }
        if survivors(index) {
            let slot = best.entry(&generation.key).or_insert(index);
            let rank = |g: &Generation| (g.published, g.scope.clone());
            if rank(generation) > rank(&generations[*slot]) {
                *slot = index;
            }
        }
    }
    best.into_iter()
        .filter(|(key, _)| !has_main.contains(key))
        .map(|(_, index)| index)
        .collect()
}

#[must_use]
pub fn prune_plan(now: Timestamp, generations: &[Generation]) -> PrunePlan<'_> {
    let newest_main = newest_main_per_key(generations);
    let removable = |index: usize| {
        !newest_main.contains(&index)
            && age(now, generations[index].published) >= PRUNE_FLOOR_SECONDS
    };
    let mut doomed: BTreeSet<usize> = (0..generations.len())
        .filter(|&i| removable(i) && expired(now, &generations[i]))
        .collect();

    let mut groups: BTreeMap<(&GenKey, Option<u64>), Vec<usize>> = BTreeMap::new();
    for (index, generation) in generations.iter().enumerate() {
        if doomed.contains(&index) {
            continue;
        }
        match generation.scope {
            GenScope::Main { .. } => groups
                .entry((&generation.key, None))
                .or_default()
                .push(index),
            GenScope::Pr { number, .. } => groups
                .entry((&generation.key, Some(number)))
                .or_default()
                .push(index),
            GenScope::Mg { .. } => {}
        }
    }
    for ((_, pr), mut members) in groups {
        members.sort_by(|&a, &b| newest_first(&generations[a], &generations[b]));
        let keep = if pr.is_some() { PR_KEEP } else { MAIN_KEEP };
        doomed.extend(members.into_iter().skip(keep).filter(|&i| removable(i)));
    }

    let newest_without_main =
        newest_per_key_without_main(generations, |index| !doomed.contains(&index));
    let mut queue: Vec<usize> = (0..generations.len())
        .filter(|i| !doomed.contains(i) && removable(*i) && !newest_without_main.contains(i))
        .collect();
    queue.sort_by_key(|&i| (generations[i].published, generations[i].path()));

    let mut victims: Vec<&Generation> = doomed.into_iter().map(|i| &generations[i]).collect();
    victims.sort_by_key(|g| g.path());
    PrunePlan {
        victims,
        eviction_queue: queue.into_iter().map(|i| &generations[i]).collect(),
    }
}
