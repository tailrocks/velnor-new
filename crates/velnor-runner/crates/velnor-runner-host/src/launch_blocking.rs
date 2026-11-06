// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::scale_set::EnsureError;

#[derive(Debug, thiserror::Error)]
pub enum ListenFault {
    #[error(transparent)]
    Host(#[from] HostError),

    #[error(transparent)]
    Ensure(#[from] EnsureError),
}
