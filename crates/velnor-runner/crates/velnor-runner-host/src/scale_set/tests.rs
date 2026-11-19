// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ScaleSetLookupAction, lookup_action};
use crate::journal::ScaleSetCreateState;

#[test]
fn an_organization_never_adopts_a_set_it_did_not_create() {
    for state in [ScaleSetCreateState::Prepared, ScaleSetCreateState::NoEffect] {
        assert_eq!(
            lookup_action(state, true, false),
            ScaleSetLookupAction::Adopt
        );
        assert_eq!(lookup_action(state, true, true), ScaleSetLookupAction::Hold);
    }
    assert_eq!(
        lookup_action(ScaleSetCreateState::Created, true, true),
        ScaleSetLookupAction::Verify
    );
    assert_eq!(
        lookup_action(ScaleSetCreateState::Prepared, false, true),
        ScaleSetLookupAction::Create
    );
}
