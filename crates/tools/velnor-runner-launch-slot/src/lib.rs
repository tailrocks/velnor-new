//! Launch slot occupancy over the host journal.
//!
//! A launch row occupies one slot until cleanup is proven. Extracted
//! from velnor-runner-launch; the only dependency is the host journal
//! vocabulary it observes.

mod slot;

pub use slot::{busy, holds, occupied, release_exited, running_count};
