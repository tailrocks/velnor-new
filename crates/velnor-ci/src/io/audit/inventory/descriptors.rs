// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use nix::sys::resource::{RLIM_INFINITY, Resource, getrlimit, setrlimit};

const UNLIMITED_TARGET: u64 = 10_240;

pub(in crate::io::audit) fn raise_open_file_limit() {
    let Ok((soft, hard)) = getrlimit(Resource::RLIMIT_NOFILE) else {
        return;
    };
    let target = if hard == RLIM_INFINITY {
        UNLIMITED_TARGET
    } else {
        hard
    };
    if target > soft {
        let _ = setrlimit(Resource::RLIMIT_NOFILE, target, hard);
    }
}

pub(super) fn open_file_limit() -> u64 {
    getrlimit(Resource::RLIMIT_NOFILE).map_or(0, |(soft, _)| soft)
}
