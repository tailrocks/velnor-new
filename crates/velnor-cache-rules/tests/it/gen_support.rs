// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{GenKey, GenScope, Generation, Head, Run, RunEvent, Sha, Timestamp};

pub(crate) const DAY: u64 = 24 * 3600;
pub(crate) const NOW: u64 = 100 * DAY;

pub(crate) fn sha(n: u8) -> Sha {
    let parsed = Sha::parse(&format!("{n:02x}").repeat(20));
    assert!(parsed.is_ok(), "bad sha {n}");
    parsed.unwrap_or_else(|_| unreachable!())
}

pub(crate) fn key(job: &str, root: &str, compat: &str) -> GenKey {
    let parsed = GenKey::new(job, root, compat);
    assert!(parsed.is_ok(), "bad key");
    parsed.unwrap_or_else(|_| unreachable!())
}

pub(crate) fn rust() -> GenKey {
    key("rust", "root", "0123456789abcdef")
}

pub(crate) fn main(n: u8) -> GenScope {
    GenScope::Main { sha: sha(n) }
}

pub(crate) fn pr(number: u64, run_id: u64, attempt: u32) -> GenScope {
    GenScope::Pr {
        number,
        run_id,
        attempt,
    }
}

pub(crate) fn mg(n: u8) -> GenScope {
    GenScope::Mg { sha: sha(n) }
}

pub(crate) fn generation(key: &GenKey, scope: GenScope, lock: &str, published: u64) -> Generation {
    Generation {
        key: key.clone(),
        scope,
        lock: lock.to_owned(),
        born: Timestamp(published),
        published: Timestamp(published),
    }
}

pub(crate) fn run(event: RunEvent, n: u8) -> Run {
    Run {
        event,
        default_branch: "main".to_owned(),
        sha: sha(n),
        run_id: 900,
        attempt: 2,
        protected_ref: true,
    }
}

pub(crate) fn push(branch: &str, n: u8) -> Run {
    run(
        RunEvent::Push {
            branch: branch.to_owned(),
        },
        n,
    )
}

pub(crate) fn pull(number: u64, n: u8) -> Run {
    run(
        RunEvent::PullRequest {
            number,
            base_branch: "main".to_owned(),
            head: Head::SameRepository,
        },
        n,
    )
}

pub(crate) fn queue(n: u8) -> Run {
    run(
        RunEvent::MergeGroup {
            queue_branch: "gh-readonly-queue/main/pr-7".to_owned(),
        },
        n,
    )
}

pub(crate) fn fork(number: u64, n: u8) -> Run {
    run(
        RunEvent::PullRequest {
            number,
            base_branch: "main".to_owned(),
            head: Head::Fork,
        },
        n,
    )
}
