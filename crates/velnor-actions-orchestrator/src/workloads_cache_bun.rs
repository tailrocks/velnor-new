//! Bun owns native downloaded package data; frozen installation always executes.
//!
//! Qualified Centrally pinned Bun honors `BUN_INSTALL_CACHE_DIR` before config/home paths.
//! Only a pure source producer may export: repository task mutations and private
//! downloads never become source authority. Preserve legitimate package members.

use std::collections::BTreeMap;

#[path = "workloads_cache_bun_producer.rs"]
pub(crate) mod producer;
#[path = "workloads_cache_bun_sources.rs"]
pub(crate) mod sources;

pub(crate) const STORE: &str = "${{ runner.temp }}/velnor/native/bun/install/cache";

/// Native source store, separate from tool installation and project outputs.
pub(crate) fn payload_paths() -> Vec<String> {
    vec![STORE.to_owned()]
}

/// Every consumer phase uses its explicit owned store, without export authority.
pub(crate) fn task_env() -> BTreeMap<String, String> {
    BTreeMap::from([("BUN_INSTALL_CACHE_DIR".to_owned(), STORE.to_owned())])
}
