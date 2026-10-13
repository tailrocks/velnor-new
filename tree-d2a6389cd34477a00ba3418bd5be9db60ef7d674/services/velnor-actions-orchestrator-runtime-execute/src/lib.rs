//! Qualified execution of one named check, through the plan/report gate.
//!
//! [`execute`] loads the staged plan and source configuration, binds
//! the discovered check to the planner's derivation, runs it under
//! its deadline, and publishes the ordinary task and matrix reports
//! plus the execution receipt. The hub keeps the environment entry
//! and resolves identities before delegating here.

pub mod execute;
