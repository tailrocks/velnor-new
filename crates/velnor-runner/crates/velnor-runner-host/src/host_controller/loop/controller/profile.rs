// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::launch::startup_storage_profile;

use super::{ControllerLoop, STORAGE_PROFILE_RETRY, setup_receipt_inputs, startup};

impl ControllerLoop<'_> {
    pub(super) async fn refresh_storage_profile(&mut self) {
        if self.admission_ready || tokio::time::Instant::now() < self.next_profile_attempt {
            return;
        }
        self.next_profile_attempt = tokio::time::Instant::now() + STORAGE_PROFILE_RETRY;
        // Gate first, profile second: a successful profile alone must never reopen admission over
        // unresolved legacy rows. The config-change reset also lands here.
        if !startup::legacy_admission(
            &self.journal,
            &self.docker,
            &self.current,
            &mut self.hold_log,
        )
        .await
        {
            // Already closed: the retry only runs while `admission_ready` is false.
            return;
        }
        let profile = startup_storage_profile(
            &self.docker,
            &self.journal,
            match self.current.resource_budget() {
                Ok(budget) => budget,
                Err(error) => {
                    eprintln!("host controller storage profile refresh held: {error}");
                    return;
                }
            },
            self.current.max_jobs(),
            &self.image_authority,
            &self.worker_images,
            &self.current.docker().endpoint,
        )
        .await;
        let Some(profile) = profile else {
            eprintln!("host controller storage profile remains unavailable");
            return;
        };
        match self.journal.configure_storage_profile(profile).await {
            Ok(()) => {
                let receipts = match setup_receipt_inputs(
                    &self.connection_sessions,
                    &self.current,
                    self.selected_engine.engine_id(),
                    self.selected_engine.docker_root_digest(),
                ) {
                    Ok(receipts) => receipts,
                    Err(error) => {
                        eprintln!("host controller setup receipt refresh held: {error}");
                        return;
                    }
                };
                if let Err(error) = self.control_state.replace_setup_receipts(receipts) {
                    eprintln!("host controller setup receipt refresh held: {error}");
                    return;
                }
                self.admission_ready = true;
                self.control_state.set_admission_ready(true);
            }
            Err(error) => {
                self.control_state.set_admission_ready(false);
                eprintln!("The controller could not store the storage profile. Error: {error}.");
            }
        }
    }
}
