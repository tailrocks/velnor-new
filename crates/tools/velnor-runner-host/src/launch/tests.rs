//! Launch ordering. The scripted transport is the acquire, JIT, and ack path.

mod backfill_tests;
#[cfg(all(test, unix))]
mod busy_slot_tests;
mod capacity_tests;
#[cfg(all(test, unix))]
mod effect_tests;
mod idless_tests;
mod launch_tests;
mod scale_tests;
mod subject_tests;
mod worker_cleanup_tests;
