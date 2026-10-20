//! SQL predicate for durable launch rows that retain a capacity charge.

/// Launch rows that still hold their permit.
pub(crate) const HOLDS_ROWS_SQL: &str = concat!(
    "kind = 'launch' AND cleanup_proven = 0 AND ",
    "(state != 'failed' OR docker_id IS NOT NULL OR dind_id IS NOT NULL ",
    "OR worker_volume IS NOT NULL OR github_runner_id IS NOT NULL)"
);
