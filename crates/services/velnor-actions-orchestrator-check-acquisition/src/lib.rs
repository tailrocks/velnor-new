//! Qualified-tool acquisition: fetch, verify, extract, lay out, and observe.
//!
//! Check-preparation leaf: turns qualified tool declarations into
//! retained, observed owned installation trees ([`acquisition`]) and
//! binds the installation identity into receipts ([`tools`]) that the
//! evidence gate validates. Depends on the core leaf plus the Mise
//! and contract adapters.

pub mod acquisition;
pub mod tools;
