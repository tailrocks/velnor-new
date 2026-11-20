// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use crate::launch::steps::ScopedProvisioningOutcome;

use super::{
    ScopedPrepareFuture, authorization::ScopedHostContext, prepare_and_bind_scoped_with_provider,
    scoped_prepare_context,
};

pub(crate) fn prepare_and_bind_scoped<'a>(
    outcome: ScopedProvisioningOutcome,
    context: &ScopedHostContext<'a>,
    deadline: Instant,
) -> ScopedPrepareFuture<'a> {
    let ScopedHostContext {
        docker,
        host,
        scoped,
        secret,
        engine,
        images,
    } = *context;
    Box::pin(async move {
        let provider =
            crate::launch::resource_probe::image_provider(secret, host, engine.endpoint())?;
        prepare_and_bind_scoped_with_provider(
            outcome,
            scoped_prepare_context(docker, scoped, host, &provider, engine, images, deadline),
        )
        .await
    })
}
