// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;
use velnor_actions_contract::QualificationSourceDelta;

use super::api::FetchedNode;

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdmissionDocument {
    pub(super) predecessor: FetchedNode,
    pub(super) source_delta: Option<QualificationSourceDelta>,
}
