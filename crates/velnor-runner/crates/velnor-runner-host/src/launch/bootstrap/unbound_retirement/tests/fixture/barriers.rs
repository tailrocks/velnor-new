// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[derive(Clone)]
pub(crate) struct CreateSubmission {
    pub(crate) name: String,
    pub(crate) labels: Value,
}

#[derive(Default)]
pub(super) struct CreateGate {
    pub(super) submission: Mutex<Option<CreateSubmission>>,
    pub(super) accepted: Notify,
    pub(super) release: Notify,
    pub(super) published: Notify,
}

pub(crate) struct CreateBarrier(pub(super) Arc<CreateGate>);

impl CreateBarrier {
    pub(crate) async fn accepted(&self) -> CreateSubmission {
        self.0.accepted.notified().await;
        self.0
            .submission
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .expect("accepted create submission")
    }

    pub(crate) fn release(&self) {
        self.0.release.notify_one();
    }

    pub(crate) async fn published(&self) {
        self.0.published.notified().await;
    }
}

#[derive(Default)]
pub(super) struct DeleteGate {
    pub(super) reached: Notify,
    pub(super) release: Notify,
}

pub(crate) struct DeleteBarrier(pub(super) Arc<DeleteGate>);

impl DeleteBarrier {
    pub(crate) async fn reached(&self) {
        self.0.reached.notified().await;
    }

    pub(crate) fn release(&self) {
        self.0.release.notify_one();
    }
}
