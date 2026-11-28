// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::entry::Timestamp;
use crate::generation::key::GenKey;
use crate::generation::scope::{GenScope, Generation};

const HOUR: u64 = 60 * 60;
const DAY: u64 = 24 * HOUR;
const GIB: u64 = 1024 * 1024 * 1024;

pub const PRUNE_FLOOR_SECONDS: u64 = 6 * HOUR;
pub const MAIN_KEEP: usize = 8;
pub const PR_KEEP: usize = 2;
pub const MAIN_MAX_BORN_SECONDS: u64 = 7 * DAY;
pub const PR_MAX_SECONDS: u64 = 7 * DAY;
pub const MG_MAX_SECONDS: u64 = DAY;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    pub high_bytes: u64,
    pub low_bytes: u64,
}

impl Budget {
    pub const DEFAULT: Self = Self {
        high_bytes: 250 * GIB,
        low_bytes: 200 * GIB,
    };
}

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

#[must_use]
pub fn prune_plan<'a>(
    now: Timestamp,
    generations: &'a [Generation],
    budget: &Budget,
) -> Vec<&'a Generation> {
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

    let mut total = (0..generations.len())
        .filter(|i| !doomed.contains(i))
        .fold(0_u64, |sum, i| sum.saturating_add(generations[i].bytes));
    if total > budget.high_bytes {
        let mut oldest_first: Vec<usize> = (0..generations.len())
            .filter(|&i| !doomed.contains(&i) && removable(i))
            .collect();
        oldest_first.sort_by_key(|&i| (generations[i].published, generations[i].path()));
        for index in oldest_first {
            if total < budget.low_bytes {
                break;
            }
            doomed.insert(index);
            total = total.saturating_sub(generations[index].bytes);
        }
    }

    let mut victims: Vec<&Generation> = doomed.into_iter().map(|i| &generations[i]).collect();
    victims.sort_by_key(|g| g.path());
    victims
}
