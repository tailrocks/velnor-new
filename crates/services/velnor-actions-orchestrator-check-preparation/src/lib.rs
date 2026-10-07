//! Isolated check-home materialization plus container evidence.
//!
//! Check-preparation phase: turns a discovered check into an owned,
//! credential-free tool home ([`preparation`]: Mise binary projection,
//! container preparation, qualified-tool acquisition) and binds the
//! container observation into receipts ([`container_receipts`]) that
//! the evidence gate validates. Builds on the acquisition leaf.

pub mod container_receipts;
pub mod preparation;
