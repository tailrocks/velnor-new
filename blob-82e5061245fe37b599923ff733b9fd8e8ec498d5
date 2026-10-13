//! Negative fixture: `RefCell` borrow across `.await`
//! (rejected by `await_holding_refcell_ref = "deny"`).

use std::cell::RefCell;

async fn tick() {}

/// Violating waiter: holds the borrow across an await point.
pub async fn read() -> u32 {
    let cell = RefCell::new(41u32);
    let borrowed = cell.borrow();
    tick().await;
    *borrowed + 1
}
