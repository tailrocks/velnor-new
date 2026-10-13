//! Test-only capture of complete marked workflow renders.
//!
//! This module is available only through the `test-render-capture` feature.

use std::cell::RefCell;

/// Canonical and selected marked YAML captured at the render boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderCapture {
    /// Canonical document before the conditional sharing fallback.
    pub canonical: String,
    /// Document selected for the unchanged production size guard.
    pub selected: String,
}

thread_local! {
    static CAPTURE: RefCell<Option<RenderCapture>> = const { RefCell::new(None) };
}

/// Clear the current thread's previous render capture.
pub fn clear() {
    CAPTURE.with(|capture| *capture.borrow_mut() = None);
}

/// Take the latest complete marked render from this thread.
pub fn take() -> Option<RenderCapture> {
    CAPTURE.with(|capture| capture.borrow_mut().take())
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
