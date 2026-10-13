//! Negative fixture: intra-doc link to a private item (rejected by
//! `private_intra_doc_links = "deny"`).

fn hidden() {}

/// Link to [`hidden`] which is private.
pub fn f() -> u32 {
    1
}
