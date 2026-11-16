// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use tokio::fs;

use super::Driver;
use crate::io::probe::request;
use crate::pairs::{DECISIONS, decisions};
use crate::side::Side;
use crate::standin;

const POLL: Duration = Duration::from_secs(1);
pub(super) const CANCEL_GRACE: Duration = Duration::from_secs(60);

impl Driver<'_> {
    pub(super) async fn refusals(&self) -> usize {
        let text = fs::read_to_string(self.env.dir.join("runner").join(DECISIONS))
            .await
            .unwrap_or_default();
        decisions(&text).map_or(0, |all| all.iter().filter(|line| !line.allow).count())
    }

    pub(super) async fn refused_after(&self, before: usize) {
        while self.refusals().await <= before {
            tokio::time::sleep(POLL).await;
        }
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
