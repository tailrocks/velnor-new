// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::{Command, ExitStatus};

use nix::sys::resource::{RLIM_INFINITY, Resource, getrlimit, setrlimit};

use crate::error::CiError;

const UNLIMITED_TARGET: u64 = 10_240;

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

impl Drop for OpenFileLimit {
    fn drop(&mut self) {
        let _ = setrlimit(Resource::RLIMIT_NOFILE, self.soft, self.hard);
    }
}

impl OpenFileLimit {
    pub(in crate::io::audit) fn for_child<T>(&self, child: impl FnOnce() -> T) -> T {
        let raised = open_file_limit();
        let _ = setrlimit(Resource::RLIMIT_NOFILE, self.soft, self.hard);
        let output = child();
        let _ = setrlimit(Resource::RLIMIT_NOFILE, raised, self.hard);
        output
    }
}

pub(in crate::io::audit) fn run_build(
    limit: Option<&OpenFileLimit>,
    build: &mut Command,
) -> Result<ExitStatus, CiError> {
    let status = match limit {
        Some(limit) => limit.for_child(|| build.status()),
        None => build.status(),
    };
    status.map_err(|e| CiError::new(format!("cannot run build command: {e}")))
}

pub(super) fn open_file_limit() -> u64 {
    getrlimit(Resource::RLIMIT_NOFILE).map_or(0, |(soft, _)| soft)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOW: u64 = 256;

    fn lowered() -> (u64, u64) {
        let (_, hard) = getrlimit(Resource::RLIMIT_NOFILE).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(
            hard > LOW,
            "the hard open-file limit {hard} leaves no room to raise the soft limit from {LOW}"
        );
        setrlimit(Resource::RLIMIT_NOFILE, LOW, hard).unwrap_or_else(|e| unreachable!("{e}"));
        (LOW, hard)
    }

    #[test]
    fn dropping_the_raise_restores_the_soft_limit_the_caller_had() {
        let (soft, _) = lowered();
        let raised = raise_open_file_limit();
        assert!(open_file_limit() > soft, "the limit was not raised");
        drop(raised);
        assert_eq!(open_file_limit(), soft);
    }

    #[test]
    fn a_child_sees_the_original_limit_and_the_raise_returns_afterwards() {
        let (soft, _) = lowered();
        let Some(raised) = raise_open_file_limit() else {
            unreachable!("the limit can be read")
        };
        let during = open_file_limit();
        let inside = raised.for_child(open_file_limit);
        assert_eq!((inside, open_file_limit()), (soft, during));
    }
}
