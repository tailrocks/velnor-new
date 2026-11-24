// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The GitHub cache API: listing and deleting `actions/caches` from the cache server's index, and
//! the check of who may call it from the job port.

use hyper::HeaderMap;
use hyper::header::AUTHORIZATION;
use serde_json::json;
use tokio::fs;

use super::{ApiStub, Call};
use crate::authority::{RUNS_PATH, dispatched};
use crate::io::authority::read_ledger;
use crate::io::reply::Reply;
use crate::io::{lock_index, now, read_index, recover_tombstones, remove_if_present, write_index};
use crate::rest::{self, DEFAULT_PER_PAGE, MAX_PER_PAGE, Page};
use crate::side::Side;
use crate::standin::Run;
use crate::token::{self, Level};

impl ApiStub {
    /// GitHub lets a job list caches with `actions: read` and delete them with `actions: write`.
    /// Here the job proves it with the signed grant that is its `github.token`; any failure to
    /// prove it is a refusal. The grant counts only for a run that the driver's ledger admits on
    /// the trusted side under an event that is not a pull request, whatever event the run claims.
    pub(super) async fn job_may_administer(
        &self,
        headers: &HeaderMap,
        path: &str,
        level: Level,
    ) -> bool {
        if self.config.job_cache_admin {
            return true;
        }
        let Some(key) = &self.config.token_key else {
            return false;
        };
        let bearer = headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));
        let Some(grant) = bearer
            .and_then(|token| token::verify_job_grant(token, key, now()).ok())
            .filter(|grant| grant.allows(&rest::repository(path), level))
        else {
            return false;
        };
        match grant.runid.parse() {
            Ok(run) => self.ledger_admits(run).await,
            Err(_) => false,
        }
    }

    async fn ledger_admits(&self, run: u64) -> bool {
        let Some(folder) = &self.config.ledger else {
            return false;
        };
        let (Ok(entries), Some(runs)) = (
            read_ledger(folder).await,
            self.standin::<Vec<Run>>(RUNS_PATH).await,
        ) else {
            return false;
        };
        dispatched(&entries, Side::Trusted, &runs, run)
            .is_some_and(|entry| !entry.event.starts_with("pull_request"))
    }

    pub(super) async fn cache_list(&self, call: &Call<'_>) -> Reply {
        let Ok(index) = read_index(&self.config.cache_dir).await else {
            return Reply::with(500, json!({"message": "cache index is unreadable"}), None);
        };
        let listed = index.listed(call.site.repository, now());
        let entries = rest::cache_list(&listed, &call.params);
        let page = Page::from_query(&call.params, DEFAULT_PER_PAGE, MAX_PER_PAGE);
        Reply::with(
            200,
            json!({"total_count": entries.len(), "actions_caches": page.slice(&entries)}),
            page.link(entries.len(), call.host, call.path, &call.params),
        )
    }

    pub(super) async fn cache_delete(&self, call: &Call<'_>, id: u64) -> Reply {
        let dir = &self.config.cache_dir;
        let Ok(_lock) = lock_index(dir).await else {
            return Reply::with(500, json!({"message": "cache index is locked"}), None);
        };
        let Ok(mut index) = read_index(dir).await else {
            return Reply::with(500, json!({"message": "cache index is unreadable"}), None);
        };
        recover_tombstones(dir, &index).await;
        let Some(record) = index.delete(call.site.repository, id) else {
            return Reply::missing();
        };
        let archive = dir.join("archives").join(&record.blob);
        let tombstone = dir
            .join("archives")
            .join(format!("{}.deleted", record.blob));
        if remove_if_present(fs::rename(&archive, &tombstone).await).is_err() {
            return Reply::with(
                500,
                json!({"message": "cache archive could not be removed"}),
                None,
            );
        }
        if write_index(dir, &index).await.is_err() {
            fs::rename(&tombstone, &archive).await.ok();
            return Reply::with(500, json!({"message": "cache index is not writable"}), None);
        }
        fs::remove_file(&tombstone).await.ok();
        fs::remove_dir_all(dir.join("blocks").join(&record.blob))
            .await
            .ok();
        Reply {
            status: 204,
            body: None,
            link: None,
        }
    }
}
