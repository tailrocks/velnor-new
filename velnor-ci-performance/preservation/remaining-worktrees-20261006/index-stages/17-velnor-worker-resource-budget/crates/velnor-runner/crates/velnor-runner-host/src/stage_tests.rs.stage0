//! DinD preparation, runner start, and cleanup tests.

mod cleanup;
mod fake;
mod preparation;
mod reconcile;
mod runner;

use crate::error::HostError;
use crate::journal::LaunchIdentity;

fn identity() -> Result<LaunchIdentity, HostError> {
    LaunchIdentity::new(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        7,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "engine-test",
    )
}
