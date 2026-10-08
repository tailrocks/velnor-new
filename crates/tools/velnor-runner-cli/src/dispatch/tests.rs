//! Dispatch-level command tests.

mod connect_common;
mod connect_guard_tests;
mod connect_tests;
mod doctor_tests;
mod drain_tests;
mod path_tests;
mod status_tests;

use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DISCONNECT_STATE: AtomicUsize = AtomicUsize::new(0);

fn disconnect_state_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "velnor-disconnect-{}-{}",
        std::process::id(),
        NEXT_DISCONNECT_STATE.fetch_add(1, Ordering::Relaxed)
    ))
}
