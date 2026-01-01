//! Negative fixture: broken intra-doc link (rejected by
//! `broken_intra_doc_links = "deny"`).

/// Link to [`NoSuchItem`] which does not exist.
pub fn f() -> u32 {
    1
}
