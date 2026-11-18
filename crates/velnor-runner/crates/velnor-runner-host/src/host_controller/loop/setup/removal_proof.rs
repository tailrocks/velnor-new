// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Writes the removal-proof record of a removed connection: the legacy runners it left behind,
//! deregistered through the GitHub admin session and confirmed absent. The legacy reconcile gate
//! releases those rows only on this record, so a removed connection whose runners could not be
//! proven gone keeps admission held.

use std::collections::BTreeSet;
use std::path::Path;

use tokio::time::timeout_at;

use crate::config::RepositoryConnection;
use crate::error::HostError;
use crate::host_controller::retirement::{record_absence, recorded_pairs};
use crate::journal::Journal;
use crate::keychain;
use crate::scale_set::{RemovedRunnerProver, RunnerProver};

use super::RemovedStartup;
use crate::host_controller::connection_pat;

pub(super) async fn record_github_absence(
    startup: &RemovedStartup<'_>,
    binding: &RepositoryConnection,
) {
    let targets = match absence_targets(startup.journal, startup.state, binding).await {
        Ok(targets) if !targets.is_empty() => targets,
        Ok(_) => return,
        Err(error) => return held(binding, error),
    };
    let state = startup.state.to_path_buf();
    let owned = binding.clone();
    let work = tokio::task::spawn_blocking(move || {
        let credential = keychain::load_secret(
            owned.credential_ref.service(),
            owned.credential_ref.account(),
        )?;
        let mut prover = RemovedRunnerProver::open(connection_pat(&credential)?, &owned)
            .map_err(|_| HostError::Endpoint)?;
        prove_and_record(&state, &owned, &targets, &mut prover)
    });
    match timeout_at(startup.deadline, work).await {
        Ok(Ok(Ok(count))) => eprintln!(
            "host controller removal proof recorded connection={} runners={count}",
            binding.connection_id
        ),
        Ok(Ok(Err(error))) => held(binding, error),
        Ok(Err(_)) | Err(_) => held(binding, HostError::Endpoint),
    }
}

/// Legacy runners with no cleanup proof that this connection's record does not cover yet.
async fn absence_targets(
    journal: &Journal,
    state: &Path,
    binding: &RepositoryConnection,
) -> Result<BTreeSet<(i64, i64)>, HostError> {
    let recorded = recorded_pairs(state, binding);
    Ok(journal
        .rows()
        .await?
        .iter()
        .filter(|row| !row.cleanup_proven)
        .filter_map(|row| {
            let runner = row.github_runner_id.as_deref()?;
            let id = runner
                .parse::<i64>()
                .ok()
                .filter(|id| id.to_string() == runner)?;
            let set = row.scale_set_id?;
            (id > 0 && set > 0 && !recorded.contains(&(set, id))).then_some((set, id))
        })
        .collect())
}

/// Deregisters each target and writes the record for the ones GitHub confirms absent. A target
/// the prover cannot confirm is left out, and when none is confirmed no record is written.
pub(super) fn prove_and_record(
    state: &Path,
    binding: &RepositoryConnection,
    targets: &BTreeSet<(i64, i64)>,
    prover: &mut impl RunnerProver,
) -> Result<usize, HostError> {
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
    record_absence(state, binding, &confirmed)?;
    Ok(confirmed.len())
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
