// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    journal::{ConnectionJournal, Journal},
    launch::{ImageAuthority, SelectedEngineContext, VerifiedWorkerImages},
};

#[derive(Clone, Copy)]
pub(crate) struct ScopedHostContext<'a> {
    pub(crate) docker: &'a bollard::Docker,
    pub(crate) host: &'a Journal,
    pub(crate) scoped: &'a ConnectionJournal,
    pub(crate) secret: &'a ImageAuthority,
    pub(crate) engine: &'a SelectedEngineContext,
    pub(crate) images: &'a VerifiedWorkerImages,
}
