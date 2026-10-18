//! Test-only capture of complete marked workflow renders.
//!
//! This module is available only through the `test-render-capture` feature.

use std::cell::{Cell, RefCell};

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
    static FULL_TREE_CAPTURE: Cell<bool> = const { Cell::new(false) };
}

struct FullTreeCaptureScope(bool);

impl Drop for FullTreeCaptureScope {
    fn drop(&mut self) {
        FULL_TREE_CAPTURE.with(|enabled| enabled.set(self.0));
    }
}

/// Clear the current thread's previous render capture.
pub fn clear() {
    CAPTURE.with(|capture| *capture.borrow_mut() = None);
}

/// Take the latest complete marked render from this thread.
#[must_use]
pub fn take() -> Option<RenderCapture> {
    CAPTURE.with(|capture| capture.borrow_mut().take())
}

/// Run diagnostic capture without applying the per-file workflow limit.
///
/// Callers must validate the complete staged tree after rendering. This
/// feature exists only for the ignored render-capture test harness.
pub fn with_full_tree_capture<T>(capture: impl FnOnce() -> T) -> T {
    let previous = FULL_TREE_CAPTURE.with(|enabled| enabled.replace(true));
    let _scope = FullTreeCaptureScope(previous);
    capture()
}

pub(crate) fn full_tree_capture_enabled() -> bool {
    FULL_TREE_CAPTURE.with(Cell::get)
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
