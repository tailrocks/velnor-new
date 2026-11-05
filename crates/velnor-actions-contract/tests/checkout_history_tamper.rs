// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::step_identity::is_configured_checkout;
use velnor_actions_contract::{Step, StepKind, StepRole};

const CHECKOUT_USES: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const PUSH_EXPRESSION: &str = "${{ github.event_name == 'push' && '1' || '0' }}";
const MERGE_PARENTS_EXPRESSION: &str = "${{ github.event_name == 'push' && '1' || '2' }}";

fn checkout_with_depth(depth: &str) -> Step {
    Step {
        parallel: false,
        name: "Checkout".to_owned(),
        id: None,
        role: Some(StepRole::Checkout),
        condition: None,
        kind: StepKind::Action {
            uses: CHECKOUT_USES.to_owned(),
            with: BTreeMap::from([
                ("persist-credentials".to_owned(), "false".to_owned()),
                ("fetch-depth".to_owned(), depth.to_owned()),
            ]),
            env: BTreeMap::new(),
        },
    }
}

#[test]
fn configured_checkout_accepts_only_the_three_promised_fetch_depths() {
    for accepted in ["0", PUSH_EXPRESSION, MERGE_PARENTS_EXPRESSION] {
        assert!(
            is_configured_checkout(&checkout_with_depth(accepted), CHECKOUT_USES),
            "fetch-depth {accepted:?} must stay a configured checkout"
        );
    }
    for rejected in [
        "1",
        "2",
        "",
        "${{ github.event_name == 'push' && '1' || '1' }}",
        "${{ github.event_name != 'push' && '1' || '0' }}",
        "${{ github.event_name == 'merge_group' && '1' || '0' }}",
        "${{ github.event_name == 'push' && '2' || '0' }}",
        "${{  github.event_name == 'push' && '1' || '0' }}",
        "${{ github.event_name == 'push' && '1' || '0' }} ",
        "${{ github.event_name != 'push' && '1' || '2' }}",
        "${{ github.event_name == 'push' && '1' || '3' }}",
        "${{ github.event_name == 'push' && '2' || '2' }}",
        "${{ github.event_name == 'merge_group' && '1' || '2' }}",
        "${{ github.event_name == 'push' && '1' || '2' }} ",
    ] {
        assert!(
            !is_configured_checkout(&checkout_with_depth(rejected), CHECKOUT_USES),
            "fetch-depth {rejected:?} must not be a configured checkout"
        );
    }
}
