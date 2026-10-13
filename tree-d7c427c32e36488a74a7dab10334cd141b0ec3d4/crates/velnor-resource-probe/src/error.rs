use std::fmt::{self, Display, Formatter};
use std::io;

/// A fixed measurement or output error that never includes sampled file contents.
#[derive(Debug)]
pub enum ProbeError {
    /// A required fixed-path measurement could not be read.
    Read(&'static str, io::Error),
    /// Required input did not match the protocol's bounded numeric grammar.
    Invalid(&'static str),
    /// A checked conversion or multiplication exceeded the output type.
    Overflow(&'static str),
    /// A fixed procfs input exceeded its explicit byte cap.
    InputTooLarge(&'static str),
    /// The serialized record exceeded the stdout protocol cap.
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
