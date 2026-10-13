//! Check outcome records and their staged persistence.
//!
//! [`evidence`] carries the verified execution outcome of one
//! qualified named check and writes its execution receipt plus any
//! scenario evidence bytes under the entry's staged home. The execute
//! phase fills the outcome; this crate persists it.

pub mod evidence;
