//! Test-only capture of complete marked workflow renders.
//!
//! This module is available only through the `test-render-capture` feature.

use std::cell::RefCell;

use crate::tree::{RenderedFile, RenderedTree};

/// Canonical and selected marked YAML captured at the render boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderCapture {
    /// Canonical document before the conditional sharing fallback.
    pub canonical: String,
    /// Document selected for the unchanged production size guard.
    pub selected: String,
    /// Shared generated action/script files available at the render boundary.
    pub shared: Vec<RenderedFile>,
    /// Complete generated tree assembled before the unchanged file-size gate.
    pub tree: Option<RenderedTree>,
}

thread_local! {
    static CAPTURE: RefCell<Option<RenderCapture>> = const { RefCell::new(None) };
    static SHARED: RefCell<Vec<RenderedFile>> = const { RefCell::new(Vec::new()) };
    static FULL_TREE_CAPTURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Scoped opt-in to capture the complete tree and bypass only the earlier
/// workflow-file size check so tree assembly reaches its unchanged size gate.
///
/// This type exists only with the `test-render-capture` feature. Dropping it,
/// including during unwinding, restores the calling thread's previous state.
#[derive(Debug)]
pub struct FullTreeCaptureGuard {
    previous: bool,
}

impl Drop for FullTreeCaptureGuard {
    fn drop(&mut self) {
        FULL_TREE_CAPTURE.with(|enabled| enabled.set(self.previous));
    }
}

/// Begin one isolated full-tree diagnostic capture on the current thread.
#[must_use]
pub fn full_tree_capture_guard() -> FullTreeCaptureGuard {
    clear();
    let previous = FULL_TREE_CAPTURE.with(|enabled| enabled.replace(true));
    FullTreeCaptureGuard { previous }
}

/// Whether the current thread is collecting a full rendered tree.
pub(crate) fn full_tree_capture_enabled() -> bool {
    FULL_TREE_CAPTURE.with(std::cell::Cell::get)
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
            tree: None,
        });
    });
}

/// Retain the complete assembled tree immediately before its normal size gate.
pub(crate) fn record_tree(tree: RenderedTree) {
    if !full_tree_capture_enabled() {
        return;
    }
    CAPTURE.with(|capture| {
        if let Some(capture) = capture.borrow_mut().as_mut() {
            capture.tree = Some(tree);
        }
    });
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::{full_tree_capture_enabled, full_tree_capture_guard};

    #[test]
    fn capture_guard_resets_after_normal_drop_and_unwind() {
        assert!(!full_tree_capture_enabled());
        {
            let _guard = full_tree_capture_guard();
            assert!(full_tree_capture_enabled());
        }
        assert!(!full_tree_capture_enabled());

        let panic = catch_unwind(AssertUnwindSafe(|| {
            let _guard = full_tree_capture_guard();
            assert!(full_tree_capture_enabled());
            panic!("exercise scoped capture cleanup");
        }));
        assert!(panic.is_err());
        assert!(!full_tree_capture_enabled());
    }
}
