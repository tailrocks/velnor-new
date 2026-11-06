// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;

use crate::error::HostError;

pub(super) async fn validated_then_ack<V, A, F>(
    validate: V,
    acknowledge: A,
) -> Result<(), HostError>
where
    V: FnOnce() -> Result<(), HostError>,
    A: FnOnce() -> F,
    F: Future<Output = Result<(), HostError>>,
{
    validate()?;
    acknowledge().await
}
