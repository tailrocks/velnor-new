// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::entry::Timestamp;
use crate::generation::key::{GenKey, Sha};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GenScope {
    Main {
        sha: Sha,
    },
    Pr {
        number: u64,
        run_id: u64,
        attempt: u32,
    },
    Mg {
        sha: Sha,
    },
}

impl GenScope {
    #[must_use]
    pub fn path(&self) -> String {
        match self {
            Self::Main { sha } => format!("main/{}", sha.as_str()),
            Self::Pr {
                number,
                run_id,
                attempt,
            } => format!("pr{number}/{run_id}-{attempt}"),
            Self::Mg { sha } => format!("mg/{}", sha.as_str()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generation {
    pub key: GenKey,
    pub scope: GenScope,
    pub lock: String,
    pub born: Timestamp,
    pub published: Timestamp,
}

impl Generation {
    #[must_use]
    pub fn path(&self) -> String {
        format!("{}/{}", self.key.dir_name(), self.scope.path())
    }
}
