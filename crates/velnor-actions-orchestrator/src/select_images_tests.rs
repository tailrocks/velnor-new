//! Claim derivation and the pure selection rules for image jobs.

use std::collections::BTreeSet;

use super::{Claims, source_root};

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn sources_claim_their_literal_directory_prefix() {
    assert_eq!(source_root("app"), "app");
    assert_eq!(source_root("assets/"), "assets");
    assert_eq!(source_root("src/*.rs"), "src");
    assert_eq!(source_root("a/b/c*/d"), "a/b");
    assert_eq!(source_root("*.sh"), "");
    assert_eq!(source_root("a?c"), "");
    assert_eq!(source_root("."), ".");
}

#[test]
fn a_claim_matches_itself_and_its_subtree_only() {
    let mut claims = Claims::default();
    claims.claim("svc/web/assets");
    claims.claim("svc/web/Dockerfile");
    assert!(claims.touched_by(&set(&["svc/web/assets"])));
    assert!(claims.touched_by(&set(&["svc/web/assets/logo/a.png"])));
    assert!(claims.touched_by(&set(&["svc/web/Dockerfile"])));
    assert!(!claims.touched_by(&set(&["svc/web/assets2/a.png"])));
    assert!(!claims.touched_by(&set(&["svc/web/README.md", "docs/a.md"])));
    assert!(!claims.touched_by(&set(&[])));
}

#[test]
fn the_context_root_claims_every_changed_path() {
    let mut claims = Claims::default();
    claims.claim(".");
    assert!(claims.everything);
    assert!(claims.touched_by(&set(&["any/where.txt"])));
    assert!(
        !claims.touched_by(&set(&[])),
        "an empty change set touches nothing"
    );
}
