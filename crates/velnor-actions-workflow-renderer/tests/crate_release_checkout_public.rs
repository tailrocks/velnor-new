// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use velnor_actions_workflow_renderer::steps::action_step;

#[test]
fn checkout_admission_expression_is_scoped_to_pinned_checkout() {
    let checkout = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
    let admitted = "${{ steps.release-admission.outputs.source_sha }}";
    let with_ref = |value: &str| BTreeMap::from([("ref".to_owned(), value.to_owned())]);
    assert!(action_step("Checkout approved source", checkout, with_ref(admitted)).is_ok());
    assert!(
        action_step(
            "Reject event input",
            checkout,
            with_ref("${{ github.event.inputs.source_sha }}")
        )
        .is_err()
    );
    assert!(
        action_step(
            "Reject other output",
            checkout,
            with_ref("${{ steps.untrusted.outputs.source_sha }}")
        )
        .is_err()
    );
    assert!(
        action_step(
            "Reject untrusted action",
            "owner/action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            with_ref(admitted)
        )
        .is_err()
    );
}
