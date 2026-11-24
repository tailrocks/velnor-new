// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Settles the legacy rows of a removed connection: the legacy runners it left behind are
//! deregistered through the GitHub admin session and, once GitHub confirms them absent, their
//! journal rows are marked cleaned. The legacy reconcile gate then releases them like any other
//! settled row; a removed connection whose runners could not be proven gone keeps admission held.

use std::collections::BTreeSet;
use std::path::Path;

use tokio::time::{Instant, timeout_at};

use crate::config::RepositoryConnection;
use crate::error::HostError;
use crate::host_controller::retirement::proof_is_current;
use crate::journal::Journal;
use crate::scale_set::{RemovedRunnerProver, RunnerProver};

use super::RemovedStartup;
use crate::host_controller::connection_pat;

pub(super) async fn record_github_absence(
    startup: &RemovedStartup<'_>,
    binding: &RepositoryConnection,
) {
    let owned = binding.clone();
    let open = move || {
        let credential = owned.credential_ref.load()?;
        RemovedRunnerProver::open(connection_pat(&credential)?, &owned)
            .map_err(|_| HostError::Endpoint)
    };
    match settle_removed(
        startup.journal,
        startup.state,
        binding,
        startup.deadline,
        open,
    )
    .await
    {
        Ok(0) => {}
        Ok(count) => eprintln!(
            "host controller removal proof settled connection={} runners={count}",
            binding.connection_id
        ),
        Err(error) => held(binding, error),
    }
}

pub(super) async fn settle_removed<P, F>(
    journal: &Journal,
    state: &Path,
    binding: &RepositoryConnection,
    deadline: Instant,
    open: F,
) -> Result<usize, HostError>
where
    P: RunnerProver,
    F: FnOnce() -> Result<P, HostError> + Send + 'static,
{
    if !proof_is_current(state, binding)? {
        return Ok(0);
    }
    let targets = absence_targets(journal).await?;
    if targets.is_empty() {
        return Ok(0);
    }
    let owned = binding.clone();
    let work = tokio::task::spawn_blocking(move || {
        let mut prover = open()?;
        Ok::<_, HostError>(prove_absent_pairs(&owned, &targets, &mut prover))
    });
    let confirmed = match timeout_at(deadline, work).await {
        Ok(Ok(Ok(confirmed))) => confirmed,
        Ok(Ok(Err(error))) => return Err(error),
        Ok(Err(_)) | Err(_) => return Err(HostError::Endpoint),
    };
    settle_rows(journal, &confirmed).await
}

fn pair(row: &crate::reconcile::IntentRow) -> Option<(i64, i64)> {
    let runner = row.github_runner_id.as_deref()?;
    let id = runner
        .parse::<i64>()
        .ok()
        .filter(|id| id.to_string() == runner)?;
    let set = row.scale_set_id?;
    (id > 0 && set > 0).then_some((set, id))
}

/// Legacy runners with no cleanup proof yet.
async fn absence_targets(journal: &Journal) -> Result<BTreeSet<(i64, i64)>, HostError> {
    Ok(journal
        .rows()
        .await?
        .iter()
        .filter(|row| !row.cleanup_proven)
        .filter_map(pair)
        .collect())
}

/// Deregisters each target and returns the ones GitHub confirms absent. A target the prover
/// cannot confirm is left out.
fn prove_absent_pairs(
    binding: &RepositoryConnection,
    targets: &BTreeSet<(i64, i64)>,
    prover: &mut impl RunnerProver,
) -> BTreeSet<(i64, i64)> {
    let mut confirmed = BTreeSet::new();
    for &(set, runner) in targets {
        match prover.prove_absent(set, runner) {
            Ok(true) => {
                confirmed.insert((set, runner));
            }
            Ok(false) => {}
            Err(error) => eprintln!(
                "host controller removal proof withheld connection={} scale_set={set} runner={runner} reason={error}",
                binding.connection_id
            ),
        }
    }
    confirmed
}

async fn settle_rows(
    journal: &Journal,
    confirmed: &BTreeSet<(i64, i64)>,
) -> Result<usize, HostError> {
    let mut settled = 0;
    for row in journal.rows().await? {
        if !row.cleanup_proven && pair(&row).is_some_and(|pair| confirmed.contains(&pair)) {
            journal.record_cleanup(row.id).await?;
            settled += 1;
        }
    }
    Ok(settled)
}

fn held(binding: &RepositoryConnection, error: HostError) {
    eprintln!(
        "host controller removal proof held connection={} reason={error}",
        binding.connection_id
    );
}

#[cfg(test)]
#[path = "removal_proof/tests.rs"]
mod tests;
