// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::{self, Display, Formatter};
use std::io;

#[derive(Debug)]
pub enum ProbeError {
    Read(&'static str, io::Error),

    Invalid(&'static str),

    Overflow(&'static str),

    InputTooLarge(&'static str),

    OutputTooLarge,
}

impl Display for ProbeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(name, _) => write!(formatter, "{name}_read_failed"),
            Self::Invalid(name) => write!(formatter, "{name}_invalid"),
            Self::Overflow(name) => write!(formatter, "{name}_overflow"),
            Self::InputTooLarge(name) => write!(formatter, "{name}_too_large"),
            Self::OutputTooLarge => formatter.write_str("output_too_large"),
        }
    }
}

impl std::error::Error for ProbeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(_, error) => Some(error),
            Self::Invalid(_)
            | Self::Overflow(_)
            | Self::InputTooLarge(_)
            | Self::OutputTooLarge => None,
        }
    }
}
