//! Bounded attempt scan for the actual runner identity in a lifecycle event.

use std::collections::HashSet;

use crate::{ActionsJob, SessionError, Transport};

use super::read::{AttemptJobs, Read, attempt_page};
use super::{ActionsJobReconciliationReason as Reason, MAX_PAGES_PER_ATTEMPT, PAGE_SIZE};

#[derive(Clone, Copy)]
pub(super) struct RunnerIdentity<'a> {
    pub(super) id: i64,
    pub(super) name: &'a str,
}

pub(super) enum AttemptLookup {
    Unique { attempt: u32, job: ActionsJob },
    NotFound,
    None,
    Mismatch,
    Multiple,
    Unknown(Reason),
}

pub(super) fn locate_observed_job<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    run_id: i64,
    runner: RunnerIdentity<'_>,
    latest_attempt: u32,
    token: &str,
) -> Result<AttemptLookup, SessionError>
where
    T: Transport + ?Sized,
{
    let mut matches = Vec::new();
    let mut partial_identity_match = false;
    for attempt in 1..=latest_attempt {
        let Read::Found(first_page) =
            attempt_page(transport, owner, repository, run_id, attempt, 1, token)?
        else {
            return Ok(AttemptLookup::NotFound);
        };
        let total_count = first_page.total_count;
        let pages = total_count.div_ceil(PAGE_SIZE).max(1);
        if pages > MAX_PAGES_PER_ATTEMPT {
            return Ok(AttemptLookup::Unknown(Reason::PaginationLimitExceeded));
        }
        let query = AttemptQuery {
            owner,
            repository,
            run_id,
            attempt,
            token,
        };
        let mut seen_job_ids = HashSet::with_capacity(total_count);
        let mut page_one = Some(first_page);
        for page_number in 1..=pages {
            let Read::Found(page) = read_page(transport, &query, page_number, &mut page_one)?
            else {
                return Ok(AttemptLookup::NotFound);
            };
            if page.total_count != total_count || page.jobs.len() > PAGE_SIZE {
                return Ok(AttemptLookup::Unknown(Reason::IncompleteAttemptPage));
            }
            let expected_count = total_count
                .saturating_sub((page_number - 1) * PAGE_SIZE)
                .min(PAGE_SIZE);
            if page.jobs.len() != expected_count {
                return Ok(AttemptLookup::Unknown(Reason::IncompleteAttemptPage));
            }
            if let Some(finding) = collect_jobs(
                page.jobs,
                run_id,
                &runner,
                attempt,
                &mut matches,
                &mut partial_identity_match,
                &mut seen_job_ids,
            ) {
                return Ok(finding);
            }
        }
        if seen_job_ids.len() != total_count {
            return Ok(AttemptLookup::Unknown(Reason::IncompleteAttemptPage));
        }
    }
    Ok(finish_matches(matches, partial_identity_match))
}

struct AttemptQuery<'a> {
    owner: &'a str,
    repository: &'a str,
    run_id: i64,
    attempt: u32,
    token: &'a str,
}

fn read_page<T>(
    transport: &mut T,
    query: &AttemptQuery<'_>,
    page: usize,
    first_page: &mut Option<AttemptJobs>,
) -> Result<Read<AttemptJobs>, SessionError>
where
    T: Transport + ?Sized,
{
    if let Some(first) = first_page.take() {
        return Ok(Read::Found(first));
    }
    attempt_page(
        transport,
        query.owner,
        query.repository,
        query.run_id,
        query.attempt,
        page,
        query.token,
    )
}

fn collect_jobs(
    jobs: Vec<ActionsJob>,
    run_id: i64,
    runner: &RunnerIdentity<'_>,
    attempt: u32,
    matches: &mut Vec<(u32, ActionsJob)>,
    partial_identity_match: &mut bool,
    seen_job_ids: &mut HashSet<i64>,
) -> Option<AttemptLookup> {
    for job in jobs {
        if job.id <= 0 || job.status.is_empty() {
            return Some(AttemptLookup::Unknown(Reason::IncompleteAttemptPage));
        }
        if !seen_job_ids.insert(job.id) {
            return Some(AttemptLookup::Unknown(Reason::DuplicateAttemptJobId));
        }
        if job.run_id != run_id {
            return Some(AttemptLookup::Unknown(Reason::AttemptRunIdMismatch));
        }
        let id_matches = job.runner_id == Some(runner.id);
        let name_matches = job.runner_name.as_deref() == Some(runner.name);
        if id_matches && name_matches {
            matches.push((attempt, job));
        } else if id_matches || name_matches {
            *partial_identity_match = true;
        }
    }
    None
}

fn finish_matches(matches: Vec<(u32, ActionsJob)>, partial_identity_match: bool) -> AttemptLookup {
    if partial_identity_match {
        return AttemptLookup::Mismatch;
    }
    let mut matches = matches.into_iter();
    let Some((attempt, job)) = matches.next() else {
        return AttemptLookup::None;
    };
    if matches.next().is_some() {
        AttemptLookup::Multiple
    } else {
        AttemptLookup::Unique { attempt, job }
    }
}
