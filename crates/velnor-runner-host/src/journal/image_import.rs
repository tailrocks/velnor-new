// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod identity;
mod inspection;
pub(in crate::journal) mod registry;

pub(crate) use identity::{ImageImportIdentity, ImageImportRequest};
pub(crate) use inspection::ImageImportObservation;

use crate::error::HostError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageImportState {
    Prepared,

    Requested,

    Uncertain,

    Completed,

    Aborted,
}

impl ImageImportState {
    pub(super) fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "requested" => Ok(Self::Requested),
            "uncertain" => Ok(Self::Uncertain),
            "completed" => Ok(Self::Completed),
            "aborted" => Ok(Self::Aborted),
            _ => Err(HostError::Journal),
        }
    }
}
