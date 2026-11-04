// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
};

struct Registry {
    replies: RefCell<VecDeque<CrateRegistryResponse>>,
    paths: RefCell<Vec<String>>,
}
impl CrateRegistryRead for Registry {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        self.paths.borrow_mut().push(path.to_owned());
        self.replies
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| internal("fixture_exhausted"))
    }
}
fn response(status: u16, body: &str) -> CrateRegistryResponse {
    CrateRegistryResponse {
        status,
        body: body.as_bytes().to_vec(),
    }
}
fn fixture(replies: Vec<CrateRegistryResponse>) -> Registry {
    Registry {
        replies: RefCell::new(replies.into()),
        paths: RefCell::new(Vec::new()),
    }
}

#[test]
fn sparse_index_waits_only_for_propagation_and_binds_checksum() {
    let checksum = "a".repeat(64);
    let matching =
        serde_json::json!({"name":"demo","vers":"0.1.1","cksum":checksum,"yanked":false})
            .to_string();
    let registry = fixture(vec![response(404, ""), response(200, &matching)]);
    let waits = Cell::new(0);
    wait_for_crate_index(&registry, "demo", "0.1.1", &checksum, 2, || {
        waits.set(waits.get() + 1);
    })
    .expect("propagation");
    assert_eq!(waits.get(), 1);
    assert_eq!(
        &*registry.paths.borrow(),
        &["/index/de/mo/demo", "/index/de/mo/demo"]
    );
    for body in [
        matching.replace(&checksum, &"b".repeat(64)),
        matching.replace("false", "true"),
        format!("{matching}\n{matching}"),
        format!(
            r#"{{"name":"demo","vers":"0.1.1","vers":"0.1.2","cksum":"{checksum}","yanked":false}}"#
        ),
        "not-json".to_owned(),
    ] {
        let registry = fixture(vec![response(200, &body)]);
        assert!(
            wait_for_crate_index(&registry, "demo", "0.1.1", &checksum, 2, || panic!(
                "invalid response retried"
            ))
            .is_err()
        );
    }
    for status in [403, 429, 500] {
        let registry = fixture(vec![response(status, "")]);
        assert!(
            wait_for_crate_index(&registry, "demo", "0.1.1", &checksum, 2, || panic!(
                "error retried"
            ))
            .is_err()
        );
    }
    let registry = fixture(vec![response(200, ""), response(404, "")]);
    assert!(wait_for_crate_index(&registry, "demo", "0.1.1", &checksum, 2, || {}).is_err());
}
