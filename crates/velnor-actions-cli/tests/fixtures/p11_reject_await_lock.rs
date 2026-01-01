//! Negative fixture: `Mutex` guard across `.await`
//! (rejected by `await_holding_lock = "deny"`).

use std::sync::Mutex;

static SLOT: Mutex<u32> = Mutex::new(0);

async fn tick() {}

/// Violating waiter: holds the guard across an await point.
pub async fn bump() -> u32 {
    let guard = SLOT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    tick().await;
    *guard + 1
}
