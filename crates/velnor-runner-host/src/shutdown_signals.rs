// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use tokio::signal::unix::{Signal, SignalKind, signal};

use crate::error::HostError;

pub(crate) struct Signals {
    terminate: Signal,
    interrupt: Signal,
}

impl Signals {
    pub(crate) fn install() -> Result<Self, HostError> {
        Ok(Self {
            terminate: signal(SignalKind::terminate()).map_err(|_| HostError::Journal)?,
            interrupt: signal(SignalKind::interrupt()).map_err(|_| HostError::Journal)?,
        })
    }

    pub(crate) async fn received(&mut self) {
        tokio::select! {
            _ = self.terminate.recv() => {}
            _ = self.interrupt.recv() => {}
        }
    }
}
