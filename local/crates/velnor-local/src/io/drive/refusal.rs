// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use tokio::fs;

use super::Driver;
use crate::io::env::Engine;
use crate::io::probe::request;
use crate::pairs::{DECISIONS, decisions, refused_among};
use crate::side::Side;
use crate::standin;

const POLL: Duration = Duration::from_secs(1);
pub(super) const CANCEL_GRACE: Duration = Duration::from_secs(60);

impl<E: Engine + Copy + Send> Driver<'_, E> {
    pub(super) async fn refused_after(&self, side: Side, event: &str, known: &[standin::Run]) {
        loop {
            if self.refused_here(side, event, known).await {
                return;
            }
            tokio::time::sleep(POLL).await;
        }
    }

    async fn refused_here(&self, side: Side, event: &str, known: &[standin::Run]) -> bool {
        let Ok(runs) = self
            .get::<Vec<standin::Run>>(side, "/_apis/v1/Message/workflow/runs")
            .await
        else {
            return false;
        };
        let ours: Vec<u64> = runs
            .iter()
            .filter(|run| !known.iter().any(|old| old.id == run.id))
            .filter(|run| run.event_name.as_deref().is_none_or(|name| name == event))
            .map(|run| run.id)
            .collect();
        let text = fs::read_to_string(self.env.dir.join("runner").join(DECISIONS))
            .await
            .unwrap_or_default();
        decisions(&text).is_ok_and(|all| refused_among(&all, side == Side::Trusted, &ours))
    }

    pub(super) async fn cancel_unfinished(&self, side: Side, known: &[standin::Run]) {
        let Ok(proxy) = self.addresses.standin(side) else {
            return;
        };
        let Ok(runs) = self
            .get::<Vec<standin::Run>>(side, "/_apis/v1/Message/workflow/runs")
            .await
        else {
            return;
        };
        for run in runs
            .iter()
            .filter(|run| run.result.is_none() && !known.iter().any(|old| old.id == run.id))
        {
            let path = format!("/_apis/v1/Message/cancelWorkflow/{}", run.id);
            request(proxy, "POST", &path, "").await;
        }
    }
}
