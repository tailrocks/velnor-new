// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use std::cell::RefCell;

thread_local! {
    static TEST_ORIGINS: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

pub(super) fn api_origin() -> Option<String> {
    TEST_ORIGINS.with(|origins| origins.borrow().as_ref().map(|value| value.0.clone()))
}

pub(super) fn storage_uri(value: &str) -> Option<Result<String, OrchestratorError>> {
    let storage = TEST_ORIGINS.with(|origins| origins.borrow().as_ref().map(|value| value.1.clone()))?;
    let valid = value.strip_prefix(&storage).is_some_and(|path| {
        path.strip_prefix("/storage/").is_some_and(|name| {
            !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    });
    Some(if valid { Ok(value.to_owned()) } else { Err(internal("github_evidence_archive_location")) })
}

pub(crate) fn with_test_origins<T>(api: &str, storage: &str, operation: impl FnOnce() -> T) -> T {
    fn loopback(origin: &str) -> bool {
        origin.strip_prefix("http://127.0.0.1:").and_then(|value| value.parse::<u16>().ok()).is_some_and(|port| port != 0 && format!("http://127.0.0.1:{port}") == origin)
    }
    assert!(loopback(api) && loopback(storage) && api != storage);
    TEST_ORIGINS.with(|origins| {
        assert!(origins.borrow().is_none());
        origins.replace(Some((api.to_owned(), storage.to_owned())));
    });
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) { TEST_ORIGINS.with(|origins| { origins.replace(None); }); }
    }
    let _reset = Reset;
    operation()
}
