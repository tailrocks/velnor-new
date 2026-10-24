//! This fixture records the historical version-13 journal schema.
//!
//! Source commit: `a72c408806c508b1c055211689c4cca350a48937`.
//! Source file: `crates/velnor-runner/crates/velnor-runner-host/src/journal/schema.rs`.
//! Source SHA-256: `2d36c29362e58d8ab972ae2a7f3610e2117815bd93c8fa90dcb0f77449672278`.
//! Source SHA-256: `6b14d45516e47bca3f2d03b38cbe4921b1b84bc78c65ec760eb9e8cb0a81829a` (`crates/velnor-runner/crates/velnor-runner-host/src/journal/schema/connection.rs`).
//! Source SHA-256: `1394c1c2eee4f2c5bab8c87467fd90e0b7c461fb3d178c29eae881f6d0dff9d2` (`crates/velnor-runner/crates/velnor-runner-host/src/journal/schema/image_import.rs`).
//! Source SHA-256: `5a9d04162e01e298db91cf27b271f58845ab6535da0ff4ed22f569fa6737f6f1` (`crates/velnor-runner/crates/velnor-runner-host/src/journal/schema/remote.rs`).
//! Source SHA-256: `daf80b6aeb0f9dc20b860134fa6bdddd405bb270b14aa1d42354b3623efd1e5d` (`crates/velnor-runner/crates/velnor-runner-host/src/journal/schema/request_ledger.rs`).
//! Source SHA-256: `6526be37b51c9d096833a46597f13943f66446ebb4df5af6f18a2eb7e1f4e6e5` (`crates/velnor-runner/crates/velnor-runner-host/src/journal/schema/scale_set_create.rs`).
//! Source SHA-256: `1e813f426af91214b825560cd8b62edf97c1da4b0e4d9e94b327e10ba26595d6` (`crates/velnor-runner/crates/velnor-runner-host/src/journal/schema/session.rs`).
//! Source SHA-256: `cba7889425e326074d8f8dc9a78387e1ed3252a5081ecbd7861c0b0adf0bcc3f` (`crates/velnor-runner/crates/velnor-runner-host/src/journal/schema/source_authorization.rs`).
//! Source SHA-256: `6ca4c3ba0da575bc2ecb1ff1f8c7b97bcba69eea61cb052f9fed6613c93853ab` (`crates/velnor-runner/crates/velnor-runner-host/src/journal/capacity_header/storage.rs`).
//! Source SHA-256: `17eeab79fc7b9c28cf17c66db5f186aaf7833914ef38cd4e9cdd1b1af2521089` (`crates/velnor-runner/crates/velnor-runner-host/src/journal_schema/resource_probe.rs`).
//! Source SHA-256: `2fb0e0451b11021abafdf6efc60032bab9dab12bcad737502da3bd7ee45f9f95` (`crates/velnor-runner/crates/velnor-runner-host/src/journal_schema.rs`).
//! The DDL and rows came from the historical `Journal::open` test harness.

use crate::error::HostError;

const HISTORICAL_INTENT_ID: i64 = 1;
const SESSION_ID: &str = "remote-session-v13";
const FIXTURE_SQL: &str = include_str!("v13_fixture.sql");

pub(super) async fn seed(path: &std::path::Path) -> Result<i64, HostError> {
    let path = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    for statement in FIXTURE_SQL.split(";\n").map(str::trim) {
        if !statement.is_empty() {
            connection
                .execute(statement, ())
                .await
                .map_err(|_| HostError::Journal)?;
        }
    }
    Ok(HISTORICAL_INTENT_ID)
}

pub(super) const fn session_id() -> &'static str {
    SESSION_ID
}
