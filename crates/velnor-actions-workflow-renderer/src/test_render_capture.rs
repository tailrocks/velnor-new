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

/// Compare generated workflow documents after parsing their shared YAML form.
///
/// # Errors
///
/// Returns the parser diagnostic when either workflow document is invalid.
pub fn workflow_yaml_is_semantically_equal(
    canonical: &str,
    selected: &str,
) -> Result<bool, String> {
    let canonical_file = crate::tree::RenderedFile {
        path: ".github/workflows/ci.yml".to_owned(),
        bytes: canonical.to_owned(),
    };
    let selected_file = crate::tree::RenderedFile {
        path: ".github/workflows/ci.yml".to_owned(),
        bytes: selected.to_owned(),
    };
    let canonical_yaml = crate::workflow_routing::parse_authority_document(&canonical_file)
        .map_err(|error| error.to_string())?;
    let selected_yaml = crate::workflow_routing::parse_authority_document(&selected_file)
        .map_err(|error| error.to_string())?;
    Ok(canonical_yaml == selected_yaml)
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

/// Run diagnostic capture within a nested, unwind-safe capture scope.
///
/// Production per-file size limits remain active. The caller must also check
/// the complete workflow tree after rendering.
pub fn with_full_tree_capture<T>(capture: impl FnOnce() -> T) -> T {
    let previous = FULL_TREE_CAPTURE.with(|enabled| enabled.replace(true));
    let _scope = FullTreeCaptureScope(previous);
    capture()
}

#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::{full_tree_capture_enabled, with_full_tree_capture};

    #[test]
    fn full_tree_capture_scope_restores_nested_state() {
        assert!(!full_tree_capture_enabled());

        let result = with_full_tree_capture(|| {
            assert!(full_tree_capture_enabled());
            with_full_tree_capture(|| assert!(full_tree_capture_enabled()));
            assert!(full_tree_capture_enabled());
            7
        });

        assert_eq!(result, 7);
        assert!(!full_tree_capture_enabled());
    }

    #[test]
    fn full_tree_capture_scope_restores_state_after_panic() {
        assert!(!full_tree_capture_enabled());

        let result = catch_unwind(AssertUnwindSafe(|| {
            with_full_tree_capture(|| panic!("test unwind"));
        }));

        assert!(result.is_err());
        assert!(!full_tree_capture_enabled());
    }
}
