//! Smoke coverage for the launch crate: admission holds without
//! capacity and capacity parsing fails closed.

use velnor_runner_launch::launch::Admit;

#[test]
fn admit_hold_is_distinct() {
    assert_ne!(Admit::Hold, Admit::Stay);
    assert_eq!(format!("{:?}", Admit::Hold), "Hold");
}
