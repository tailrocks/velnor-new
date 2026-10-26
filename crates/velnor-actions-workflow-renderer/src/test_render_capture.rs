// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::{Cell, RefCell};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderCapture {
    pub canonical: String,
    pub selected: String,
}

thread_local! {
    static CAPTURE: RefCell<Option<RenderCapture>> = const { RefCell::new(None) };
    static FULL_TREE_CAPTURE: Cell<bool> = const { Cell::new(false) };
}

struct FullTreeCaptureScope(bool);

impl Drop for FullTreeCaptureScope {
    fn drop(&mut self) {
        FULL_TREE_CAPTURE.with(|enabled| enabled.set(self.0));
    }
}

pub fn clear() {
    CAPTURE.with(|capture| *capture.borrow_mut() = None);
}

#[must_use]
pub fn take() -> Option<RenderCapture> {
    CAPTURE.with(|capture| capture.borrow_mut().take())
}

pub fn with_full_tree_capture<T>(capture: impl FnOnce() -> T) -> T {
    let previous = FULL_TREE_CAPTURE.with(|enabled| enabled.replace(true));
    let _scope = FullTreeCaptureScope(previous);
    capture()
}

pub(crate) fn record(path: &str, canonical: String, selected: String) {
    if path != ".github/workflows/ci.yml" {
        return;
    }
    CAPTURE.with(|capture| {
        *capture.borrow_mut() = Some(RenderCapture {
            canonical,
            selected,
        });
    });
}
