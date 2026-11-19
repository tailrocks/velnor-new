// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The GitHub cache API: listing and deleting `actions/caches` from the cache server's index, and
//! the check of who may call it from the job port.

use hyper::Request;
use hyper::header::AUTHORIZATION;
use serde_json::json;
use tokio::fs;

use super::{ApiStub, Call};
use crate::io::reply::Reply;
use crate::io::{
    Body, lock_index, now, read_index, recover_tombstones, remove_if_present, write_index,
};
use crate::rest::{self, DEFAULT_PER_PAGE, MAX_PER_PAGE, Page};
use crate::token::{self, Level};

impl ApiStub {
    /// GitHub lets a job list caches with `actions: read` and delete them with `actions: write`.
    /// Here the job proves it with the signed grant that is its `github.token`; any failure to
    /// prove it is a refusal.
    pub(super) fn job_may_administer(&self, request: &Request<Body>, level: Level) -> bool {
        if self.config.job_cache_admin {
            return true;
        }
        let Some(key) = &self.config.token_key else {
            return false;
        };
        let bearer = request
            .headers()
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));
        bearer
            .and_then(|token| token::verify_job_grant(token, key, now()).ok())
            .is_some_and(|grant| grant.allows(&rest::repository(request.uri().path()), level))
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
            let _ = fs::rename(&tombstone, &archive).await;
            return Reply::with(500, json!({"message": "cache index is not writable"}), None);
        }
        let _ = fs::remove_file(&tombstone).await;
        let _ = fs::remove_dir_all(dir.join("blocks").join(&record.blob)).await;
        Reply {
            status: 204,
            body: None,
            link: None,
        }
    }
}
