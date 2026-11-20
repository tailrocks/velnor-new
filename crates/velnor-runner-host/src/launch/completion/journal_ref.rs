// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ops::Deref;

use crate::journal::ConnectionJournal;

pub(in crate::launch) struct CompletionJournal(ConnectionJournal);

impl CompletionJournal {
    pub(super) const fn scoped(journal: ConnectionJournal) -> Self {
        Self(journal)
    }

    pub(super) const fn connection(&self) -> &ConnectionJournal {
        &self.0
    }
}

impl Deref for CompletionJournal {
    type Target = ConnectionJournal;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
