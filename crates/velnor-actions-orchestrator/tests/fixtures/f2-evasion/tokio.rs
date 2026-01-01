//! Evasion fixture: async process spawn must be flagged.
//!
//! Awaits an async child; the forbidden table names
//! `tokio::process::Command` exactly.

async fn run_evil() {
    let _ = tokio::process::Command::new("evil").output().await;
}
