#[cfg(feature = "test-render-capture")]
#[path = "test_render_capture_harness.rs"]
mod test_render_capture_harness;

#[path = "tap_test_support.rs"]
pub(crate) mod tap_test_support;

#[path = "../../test_support/git_fixture.rs"]
pub(crate) mod git_fixture;
