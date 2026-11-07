// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::config::MiseCheck;

#[test]
fn closure_is_explicit_boolean_and_round_trips() -> Result<(), Box<dyn std::error::Error>> {
    let mut value = serde_json::json!({"id":"fixture","task":"fixture","runner":{"label":"ubuntu-26.04","platform":"linux_x64","executor":"hosted"},"inputs":[],"tools":[]});
    let check: MiseCheck = serde_json::from_value(value.clone())?;
    assert!(!check.checkout_blob_closure);
    assert!(
        serde_json::to_value(check)?
            .get("checkout_blob_closure")
            .is_none()
    );
    value["checkout_blob_closure"] = true.into();
    let check: MiseCheck = serde_json::from_value(value.clone())?;
    assert!(check.checkout_blob_closure);
    assert_eq!(serde_json::to_value(check)?["checkout_blob_closure"], true);
    value["checkout_blob_closure"] = "true".into();
    assert!(serde_json::from_value::<MiseCheck>(value).is_err());
    Ok(())
}
