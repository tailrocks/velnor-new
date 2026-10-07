//! Exact-base run and baseline-artifact selection, extracted from the orchestrator.
//!
//! Pure selection policy over pinned `gh` listings ([`select_exact_base_run`]
//! picks the newest matching successful push run; [`select_baseline_artifact`]
//! picks the exact unexpired artifact). Selection never guesses: entries
//! without attempt evidence or without the exact artifact never select.

mod run_select;

pub use run_select::SelectedBaseRun;
pub use run_select::select_baseline_artifact;
pub use run_select::select_exact_base_run;
