// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod provenance;

pub(crate) use provenance::{StructurallyVerifiedProvenance, verify_family_provenance};
pub(crate) use velnor_actions_contract::{ExpectedBuildSource, ImageKind, admits_build_source};
