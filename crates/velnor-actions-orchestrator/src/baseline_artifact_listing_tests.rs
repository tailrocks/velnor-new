//! Pagination and exact-name listing regressions.

use super::*;
use std::cell::RefCell;
use std::ffi::OsString;

fn artifact(id: u64, name: &str, expired: bool) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": name,
        "expired": expired,
        "size_in_bytes": 1,
        "digest": format!("sha256:{}", "a".repeat(64)),
    })
}

fn page(total_count: u64, artifacts: &[serde_json::Value]) -> String {
    serde_json::json!({"total_count": total_count, "artifacts": artifacts}).to_string()
}

fn query_args(args: &[OsString]) -> String {
    args[1].to_string_lossy().into_owned()
}

#[test]
fn exact_name_duplicate_on_later_page_rejects_candidate() {
    let name = "velnor-baseline-".to_owned() + &"a".repeat(40) + "-b3-" + &"b".repeat(64);
    let mut first_page = vec![artifact(1, &name, false)];
    first_page.extend((2..=100).map(|id| artifact(id, &name, true)));
    let second_page = vec![artifact(101, &name, false)];
    let pages = RefCell::new(Vec::new());
    let result = select_metadata("o/r", 7, &name, |args| {
        let query = query_args(&args);
        pages.borrow_mut().push(query.clone());
        if query.ends_with("page=1") {
            Ok(page(101, &first_page))
        } else {
            Ok(page(101, &second_page))
        }
    });
    assert!(result.is_err());
    let pages = pages.into_inner();
    assert_eq!(pages.len(), 2);
    assert!(
        pages
            .iter()
            .all(|query| query.contains(&format!("name={name}&per_page=100")))
    );
}

#[test]
fn valid_candidate_on_later_page_is_selected() {
    let name = "velnor-baseline-".to_owned() + &"a".repeat(40) + "-b3-" + &"b".repeat(64);
    let first_page: Vec<_> = (1..=100).map(|id| artifact(id, &name, true)).collect();
    let result = select_metadata("o/r", 7, &name, |args| {
        let query = query_args(&args);
        if query.ends_with("page=1") {
            Ok(page(101, &first_page))
        } else {
            Ok(page(101, &[artifact(101, &name, false)]))
        }
    });
    assert_eq!(
        result.expect("later page candidate").id,
        101,
        "later exact live artifact must not be missed"
    );
}

#[test]
fn duplicate_service_id_across_pages_fails_closed() {
    let name = "velnor-baseline-".to_owned() + &"a".repeat(40) + "-b3-" + &"b".repeat(64);
    let first_page: Vec<_> = (1..=100).map(|id| artifact(id, &name, id != 1)).collect();
    let result = select_metadata("o/r", 7, &name, |args| {
        let query = query_args(&args);
        if query.ends_with("page=1") {
            Ok(page(101, &first_page))
        } else {
            Ok(page(101, &[artifact(100, &name, true)]))
        }
    });
    assert!(
        result.is_err(),
        "overlapping page ID must not hide an omitted artifact"
    );
}

#[test]
fn nonpositive_id_in_expired_entry_fails_closed() {
    let name = "velnor-baseline-".to_owned() + &"a".repeat(40) + "-b3-" + &"b".repeat(64);
    let mut artifacts: Vec<_> = (1..=100).map(|id| artifact(id, &name, id != 1)).collect();
    artifacts[99]["id"] = serde_json::json!(0);
    let result = select_metadata("o/r", 7, &name, |_| Ok(page(100, &artifacts)));
    assert!(
        result.is_err(),
        "expired entries still require positive service IDs"
    );
}

#[test]
fn short_page_with_larger_total_count_fails_closed() {
    let name = "velnor-baseline-".to_owned() + &"a".repeat(40) + "-b3-" + &"b".repeat(64);
    let calls = RefCell::new(0);
    let result = select_metadata("o/r", 7, &name, |_| {
        *calls.borrow_mut() += 1;
        Ok(page(101, &[artifact(1, &name, false)]))
    });
    assert!(result.is_err());
    assert_eq!(calls.into_inner(), 1);
}

#[test]
fn changing_total_count_between_pages_fails_closed() {
    let name = "velnor-baseline-".to_owned() + &"a".repeat(40) + "-b3-" + &"b".repeat(64);
    let first_page: Vec<_> = (1..=100).map(|id| artifact(id, &name, id != 1)).collect();
    let result = select_metadata("o/r", 7, &name, |args| {
        let query = query_args(&args);
        if query.ends_with("page=1") {
            Ok(page(101, &first_page))
        } else {
            Ok(page(
                102,
                &[artifact(101, &name, true), artifact(102, &name, false)],
            ))
        }
    });
    assert!(result.is_err());
}

#[test]
fn page_cap_rejects_listing_before_unbounded_pages() {
    let name = "velnor-baseline-".to_owned() + &"a".repeat(40) + "-b3-" + &"b".repeat(64);
    let calls = RefCell::new(0);
    let first_page: Vec<_> = (1..=100).map(|id| artifact(id, &name, id != 1)).collect();
    let result = select_metadata("o/r", 7, &name, |_| {
        *calls.borrow_mut() += 1;
        Ok(page(1001, &first_page))
    });
    assert!(result.is_err());
    assert_eq!(calls.into_inner(), 1);
}
