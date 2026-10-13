//! Evasion fixture: concat-built spawn string must be flagged.
//!
//! Splits the forbidden path across string concatenation so a naive
//! substring scan misses it; normalization must still flag it.

fn run_evil() {
    let spawn = "tokio::" + "process::" + "Command::new";
    let _ = spawn;
}
