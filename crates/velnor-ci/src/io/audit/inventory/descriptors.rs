// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use nix::sys::resource::{RLIM_INFINITY, Resource, getrlimit, setrlimit};

const UNLIMITED_TARGET: u64 = 10_240;

#[derive(Clone, Copy)]
pub(in crate::io::audit) struct OpenFileLimit {
    soft: u64,
    hard: u64,
}

pub(in crate::io::audit) fn raise_open_file_limit() -> Option<OpenFileLimit> {
    let (soft, hard) = getrlimit(Resource::RLIMIT_NOFILE).ok()?;
    let target = if hard == RLIM_INFINITY {
        UNLIMITED_TARGET
    } else {
        hard
    };
    if target > soft {
        let _ = setrlimit(Resource::RLIMIT_NOFILE, target, hard);
    }
    Some(OpenFileLimit { soft, hard })
}

impl OpenFileLimit {
    pub(in crate::io::audit) fn for_child<T>(self, child: impl FnOnce() -> T) -> T {
        let raised = open_file_limit();
        let _ = setrlimit(Resource::RLIMIT_NOFILE, self.soft, self.hard);
        let output = child();
        let _ = setrlimit(Resource::RLIMIT_NOFILE, raised, self.hard);
        output
    }
}

pub(super) fn open_file_limit() -> u64 {
    getrlimit(Resource::RLIMIT_NOFILE).map_or(0, |(soft, _)| soft)
}
