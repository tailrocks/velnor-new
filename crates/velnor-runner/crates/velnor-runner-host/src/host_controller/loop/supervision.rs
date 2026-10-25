use std::{collections::HashSet, time::Duration};

use bollard::Docker;

use crate::{
    config::RepositoryConnection,
    journal::{
        Journal, LocalWorkerExitDisposition, LocalWorkerExitKind, LocalWorkerHealthCandidate,
    },
    launch::SelectedEngineContext,
};

const MAX_INSPECTIONS_PER_PASS: usize = 4;
const MAX_RESERVATION_PAGES_PER_PASS: usize = 4;
const MAX_PASS_IO: Duration = Duration::from_secs(2);
const MAX_REPORTED_EXITS_PER_CONNECTION: usize = 16;

enum ScanCursor {
    AfterReservation {
        identity: Vec<u8>,
        reservation_id: i64,
    },
    AfterScope {
        identity: Vec<u8>,
    },
}

#[derive(Default)]
pub(crate) struct LocalWorkerHealthSupervisor {
    cursor: Option<ScanCursor>,
}

impl LocalWorkerHealthSupervisor {
    pub(crate) async fn supervise(
        &mut self,
        journal: &Journal,
        docker: &Docker,
        selected: &SelectedEngineContext,
        active: &[RepositoryConnection],
        retiring: &[RepositoryConnection],
    ) {
        supervise_local_worker_health(self, journal, docker, selected, active, retiring).await;
    }
}

struct ScopeEntry {
    identity: Vec<u8>,
    scope: crate::journal::ConnectionJournal,
}

enum PageProgress {
    SameScope(i64),
    NextScope,
    Stop,
}

#[cfg(test)]
#[path = "supervision/fairness_tests.rs"]
mod fairness_tests;
#[path = "supervision/inspect.rs"]
mod inspect;
#[cfg(test)]
#[path = "supervision/tests.rs"]
mod tests;

pub(crate) async fn supervise_local_worker_health(
    supervisor: &mut LocalWorkerHealthSupervisor,
    journal: &Journal,
    docker: &Docker,
    selected: &SelectedEngineContext,
    active: &[RepositoryConnection],
    retiring: &[RepositoryConnection],
) {
    let scopes = existing_scopes(journal, active, retiring).await;
    if scopes.is_empty() {
        supervisor.cursor = None;
        return;
    }

    let scope_index = next_scope_index(&scopes, supervisor.cursor.as_ref());
    let after_reservation_id = match supervisor.cursor.as_ref() {
        Some(ScanCursor::AfterReservation {
            identity,
            reservation_id,
        }) if scopes[scope_index].identity == *identity => *reservation_id,
        _ => 0,
    };
    scan_candidates(
        supervisor,
        &scopes,
        scope_index,
        after_reservation_id,
        docker,
        selected,
    )
    .await;
}

async fn existing_scopes(
    journal: &Journal,
    active: &[RepositoryConnection],
    retiring: &[RepositoryConnection],
) -> Vec<ScopeEntry> {
    let mut scopes = Vec::new();
    for (identity, binding) in unique_bindings(active, retiring) {
        let scope = match journal.existing_connection_scope(binding).await {
            Ok(scope) => scope,
            Err(error) => {
                eprintln!(
                    "local worker health scope held: connection={} error={error}",
                    binding.connection_id.as_str()
                );
                continue;
            }
        };
        report_unresolved(&scope).await;
        scopes.push(ScopeEntry { identity, scope });
    }
    scopes
}

async fn scan_candidates(
    supervisor: &mut LocalWorkerHealthSupervisor,
    scopes: &[ScopeEntry],
    mut scope_index: usize,
    mut after_reservation_id: i64,
    docker: &Docker,
    selected: &SelectedEngineContext,
) {
    let io_deadline = tokio::time::Instant::now() + MAX_PASS_IO;
    let mut pages = 0;
    let mut inspections = 0;
    let mut stopped = false;
    while scope_index < scopes.len()
        && pages < MAX_RESERVATION_PAGES_PER_PASS
        && inspections < MAX_INSPECTIONS_PER_PASS
        && io_deadline > tokio::time::Instant::now()
    {
        let entry = &scopes[scope_index];
        let progress = scan_scope_page(
            supervisor,
            entry,
            after_reservation_id,
            docker,
            selected,
            io_deadline,
            &mut inspections,
        )
        .await;
        pages += 1;
        match progress {
            PageProgress::SameScope(reservation_id) => {
                after_reservation_id = reservation_id;
            }
            PageProgress::NextScope => {
                scope_index += 1;
                after_reservation_id = 0;
            }
            PageProgress::Stop => {
                stopped = true;
                break;
            }
        }
    }
    if scope_index >= scopes.len() && !stopped {
        supervisor.cursor = None;
    }
}

async fn scan_scope_page(
    supervisor: &mut LocalWorkerHealthSupervisor,
    entry: &ScopeEntry,
    after_reservation_id: i64,
    docker: &Docker,
    selected: &SelectedEngineContext,
    io_deadline: tokio::time::Instant,
    inspections: &mut usize,
) -> PageProgress {
    let page = entry
        .scope
        .local_worker_health_candidate_page_after(after_reservation_id)
        .await;
    let (candidates, scanned_through, has_more) = match page {
        Ok(page) => page,
        Err(error) => {
            eprintln!(
                "local worker health page held: connection={} error={error}",
                entry.scope.connection_id().as_str()
            );
            supervisor.cursor = Some(ScanCursor::AfterScope {
                identity: entry.identity.clone(),
            });
            return PageProgress::NextScope;
        }
    };
    for candidate in &candidates {
        if io_deadline <= tokio::time::Instant::now() {
            return PageProgress::Stop;
        }
        supervise_candidate(&entry.scope, docker, selected, candidate, io_deadline).await;
        *inspections += 1;
        supervisor.cursor = Some(ScanCursor::AfterReservation {
            identity: entry.identity.clone(),
            reservation_id: candidate.reservation_id(),
        });
        if *inspections >= MAX_INSPECTIONS_PER_PASS || io_deadline <= tokio::time::Instant::now() {
            return PageProgress::Stop;
        }
    }
    if has_more && let Some(scanned_through) = scanned_through {
        supervisor.cursor = Some(ScanCursor::AfterReservation {
            identity: entry.identity.clone(),
            reservation_id: scanned_through,
        });
        return PageProgress::SameScope(scanned_through);
    }
    supervisor.cursor = Some(ScanCursor::AfterScope {
        identity: entry.identity.clone(),
    });
    PageProgress::NextScope
}

fn unique_bindings<'a>(
    active: &'a [RepositoryConnection],
    retiring: &'a [RepositoryConnection],
) -> Vec<(Vec<u8>, &'a RepositoryConnection)> {
    let mut seen = HashSet::new();
    let mut bindings = Vec::new();
    for binding in active.iter().chain(retiring) {
        match binding.canonical_identity_bytes() {
            Ok(identity) => {
                if seen.insert(identity.clone()) {
                    bindings.push((identity, binding));
                }
            }
            Err(_) => eprintln!(
                "local worker health binding held: connection={}",
                binding.connection_id.as_str()
            ),
        }
    }
    bindings.sort_by(|left, right| left.0.cmp(&right.0));
    bindings
}

fn next_scope_index(scopes: &[ScopeEntry], cursor: Option<&ScanCursor>) -> usize {
    let index = match cursor {
        None => return 0,
        Some(ScanCursor::AfterReservation { identity, .. }) => {
            scopes.partition_point(|scope| scope.identity < *identity)
        }
        Some(ScanCursor::AfterScope { identity }) => {
            scopes.partition_point(|scope| scope.identity <= *identity)
        }
    };
    if index == scopes.len() {
        return 0;
    }
    index
}

async fn supervise_candidate(
    scope: &crate::journal::ConnectionJournal,
    docker: &Docker,
    selected: &SelectedEngineContext,
    candidate: &LocalWorkerHealthCandidate,
    io_deadline: tokio::time::Instant,
) {
    let engine = match scope.engine_context().await {
        Ok(Some(engine)) => engine,
        Ok(None) => {
            eprintln!(
                "local worker health inspect held: connection={} reason=docker engine binding missing; capacity=held",
                scope.connection_id().as_str()
            );
            return;
        }
        Err(error) => {
            eprintln!(
                "local worker health inspect held: connection={} error={error}; capacity=held",
                scope.connection_id().as_str()
            );
            return;
        }
    };
    let observation =
        inspect::inspect_runner(docker, &engine, selected, candidate, io_deadline).await;
    let kind = match observation {
        inspect::RunnerObservation::Running => return,
        inspect::RunnerObservation::Exited { exit_code } => {
            LocalWorkerExitKind::Exited { exit_code }
        }
        inspect::RunnerObservation::Missing => LocalWorkerExitKind::Missing,
        inspect::RunnerObservation::Held(reason) => {
            eprintln!(
                "local worker health inspect held: connection={} scale_set_id={} lineage={} generation={} reservation={} runner_container_id={} reason={reason}; capacity=held waiting_for=matching GitHub JobCompleted and cleanup/release proof",
                scope.connection_id().as_str(),
                candidate.scale_set_id(),
                candidate.lineage_id(),
                candidate.generation(),
                candidate.reservation_id(),
                candidate.runner_container_id()
            );
            return;
        }
    };
    match scope.record_local_worker_exit(candidate, kind).await {
        Ok(LocalWorkerExitDisposition::Recorded) => eprintln!(
            "local worker exit quarantined: connection={} scale_set_id={} lineage={} generation={} reservation={} runner_container_id={} kind={kind:?}; capacity=held waiting_for=matching GitHub JobCompleted and cleanup/release proof",
            scope.connection_id().as_str(),
            candidate.scale_set_id(),
            candidate.lineage_id(),
            candidate.generation(),
            candidate.reservation_id(),
            candidate.runner_container_id()
        ),
        Ok(LocalWorkerExitDisposition::AlreadyRecorded) => {}
        Ok(LocalWorkerExitDisposition::IgnoredStale) => eprintln!(
            "local worker health candidate changed before quarantine: connection={} lineage={} generation={} reservation={}",
            scope.connection_id().as_str(),
            candidate.lineage_id(),
            candidate.generation(),
            candidate.reservation_id()
        ),
        Err(error) => eprintln!(
            "local worker exit journal write held: connection={} lineage={} generation={} reservation={} error={error}; capacity=held",
            scope.connection_id().as_str(),
            candidate.lineage_id(),
            candidate.generation(),
            candidate.reservation_id()
        ),
    }
}

async fn report_unresolved(scope: &crate::journal::ConnectionJournal) {
    let exits: Vec<crate::journal::LocalWorkerExitObservation> =
        match scope.unresolved_local_worker_exits().await {
            Ok(exits) => exits,
            Err(error) => {
                eprintln!(
                    "local worker health report held: connection={} error={error}",
                    scope.connection_id().as_str()
                );
                return;
            }
        };
    for exit in exits.iter().take(MAX_REPORTED_EXITS_PER_CONNECTION) {
        eprintln!(
            "local worker exit unresolved: connection={} scale_set_id={} lineage={} generation={} reservation={} intent={} runner_container_id={} kind={:?}; capacity=held waiting_for=matching GitHub JobCompleted and cleanup/release proof",
            scope.connection_id().as_str(),
            exit.scale_set_id(),
            exit.lineage_id(),
            exit.generation(),
            exit.reservation_id(),
            exit.intent_id(),
            exit.runner_container_id(),
            exit.kind()
        );
    }
    if exits.len() > MAX_REPORTED_EXITS_PER_CONNECTION {
        eprintln!(
            "local worker exits unresolved: connection={} additional_held={} capacity=held waiting_for=matching GitHub JobCompleted and cleanup/release proof",
            scope.connection_id().as_str(),
            exits.len() - MAX_REPORTED_EXITS_PER_CONNECTION
        );
    }
}
