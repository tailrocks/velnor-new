// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::generation::key::{GenKey, Sha};
use crate::generation::run::{Lane, Run};
use crate::generation::scope::{GenScope, Generation};

#[derive(Clone, Copy, Debug)]
pub struct Want<'a> {
    pub run: &'a Run,
    pub key: &'a GenKey,
    pub lock: &'a str,
    pub base: Option<&'a Sha>,
}

fn newest<'a>(mut found: impl Iterator<Item = &'a Generation>) -> Option<&'a Generation> {
    found.next().map(|first| {
        found.fold(first, |best, next| {
            if (next.published, &next.scope) > (best.published, &best.scope) {
                next
            } else {
                best
            }
        })
    })
}

fn highest_run<'a>(found: impl Iterator<Item = &'a Generation>) -> Option<&'a Generation> {
    found.max_by(|a, b| a.scope.cmp(&b.scope))
}

#[must_use]
pub fn pick<'a>(want: &Want<'_>, generations: &'a [Generation]) -> Option<&'a Generation> {
    let own = || generations.iter().filter(|g| g.key == *want.key);
    let locked = |g: &&Generation| g.lock == want.lock;
    want.run.lanes().iter().find_map(|lane| match lane {
        Lane::Pr(number) => {
            let in_pr = || own().filter(|g| matches!(g.scope, GenScope::Pr { number: n, .. } if n == *number));
            highest_run(in_pr().filter(locked)).or_else(|| highest_run(in_pr()))
        }
        Lane::Mg(sha) => own().find(|g| matches!(&g.scope, GenScope::Mg { sha: s } if s == sha)),
        Lane::Main => {
            let main = || own().filter(|g| matches!(g.scope, GenScope::Main { .. }));
            let is_base = |g: &&Generation| {
                matches!(&g.scope, GenScope::Main { sha } if Some(sha) == want.base)
            };
            newest(main().filter(is_base).filter(locked))
                .or_else(|| newest(main().filter(locked)))
                .or_else(|| newest(main().filter(is_base)))
                .or_else(|| newest(main()))
        }
    })
}
