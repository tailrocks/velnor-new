// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod model;
mod reader;

pub(crate) use model::BoundJobFactsSnapshot;
pub(crate) use reader::{JobFactsReadContext, read_job_facts_snapshot};
