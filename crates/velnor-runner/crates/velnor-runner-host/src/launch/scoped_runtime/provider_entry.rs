// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;

use crate::{
    journal::{ConnectionJournal, Journal},
    launch::{
        SelectedEngineContext, VerifiedWorkerImages, resource_probe::ReleaseReadSecret,
        steps::ScopedProvisioningOutcome,
    },
};

use super::{ScopedPrepareFuture, prepare_and_bind_scoped_with_provider, scoped_prepare_context};

pub(crate) fn prepare_and_bind_scoped<'a>(
    outcome: ScopedProvisioningOutcome,
    docker: &'a Docker,
    scoped: &'a ConnectionJournal,
    host: &'a Journal,
    release_secret: &'a ReleaseReadSecret,
    engine: &'a SelectedEngineContext,
    images: &'a VerifiedWorkerImages,
) -> ScopedPrepareFuture<'a> {
    Box::pin(async move {
        let provider = crate::launch::resource_probe::release_image_provider(
            release_secret,
            host,
            engine.endpoint(),
        )?;
        prepare_and_bind_scoped_with_provider(
            outcome,
            scoped_prepare_context(docker, scoped, host, &provider, engine, images),
        )
        .await
    })
}
