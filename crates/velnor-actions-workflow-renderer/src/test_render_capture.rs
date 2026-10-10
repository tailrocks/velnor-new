//! Test-only capture of complete marked workflow renders.
//!
//! This module is available only through the `test-render-capture` feature.

use std::cell::RefCell;

use crate::tree::RenderedFile;

/// Canonical and selected marked YAML captured at the render boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderCapture {
    /// Canonical document before the conditional sharing fallback.
    pub canonical: String,
    /// Document selected for the unchanged production size guard.
    pub selected: String,
    /// Shared generated action/script files available at the render boundary.
    pub shared: Vec<RenderedFile>,
}

thread_local! {
    static CAPTURE: RefCell<Option<RenderCapture>> = const { RefCell::new(None) };
    static SHARED: RefCell<Vec<RenderedFile>> = const { RefCell::new(Vec::new()) };
}

/// Clear the current thread's previous render capture.
pub fn clear() {
    CAPTURE.with(|capture| *capture.borrow_mut() = None);
    SHARED.with(|shared| shared.borrow_mut().clear());
}

/// Take the latest complete marked render from this thread.
pub fn take() -> Option<RenderCapture> {
    CAPTURE.with(|capture| capture.borrow_mut().take())
}

/// Retain shared action files before the workflow-size guard can reject YAML.
pub(crate) fn record_shared(files: Vec<RenderedFile>) {
    let mut files = files;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    SHARED.with(|shared| *shared.borrow_mut() = files);
}

pub(crate) fn record(path: &str, canonical: String, selected: String) {
    if path != ".github/workflows/ci.yml" {
        return;
    }
    let shared = SHARED.with(|shared| std::mem::take(&mut *shared.borrow_mut()));
    CAPTURE.with(|capture| {
        *capture.borrow_mut() = Some(RenderCapture {
            canonical,
            selected,
            shared,
        });
    });
}
